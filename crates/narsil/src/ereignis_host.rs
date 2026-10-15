//! Browser events run on the page engine and its existing DOM host.
use crate::{
    dom,
    handler::{Handler, ListenerId},
    js,
};
use narsil_jsvm::{Engine, Wert};
use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct DispatchResult {
    pub invoked: bool,
    pub errors: Vec<String>,
    pub console: Vec<String>,
    pub commands: Vec<js::Befehl>,
}

pub(crate) fn dispatch(
    engine: &mut Engine,
    stand: &dom::Geteilt,
    kiste: &js::Geteilt,
    target: u32,
    ancestors: &[u32],
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    x: f32,
    y: f32,
    fallback: &HashMap<(u32, String), Handler>,
) -> DispatchResult {
    dom::vorgaenge_zuruecksetzen();
    engine.browser_task_beginnen();
    {
        let mut state = stand.lock().expect("DOM");
        state.default_verhindert = false;
        state.propagation_stopped = false;
        state.immediate_stopped = false;
        state.passive_listener = false;
        state.event_cancelable = Some(cancelable);
    }
    let mut result = DispatchResult::default();
    // Ancestors are ordered nearest first; target itself is supplied separately.
    let mut path = Vec::new();
    for &host in ancestors {
        if host != target && !path.contains(&host) {
            path.push(host);
        }
    }
    for &host in path.iter().rev() {
        invoke_target(
            engine,
            stand,
            kiste,
            target,
            host,
            event_type,
            1,
            true,
            bubbles,
            x,
            y,
            fallback,
            &mut result,
        );
        if stand.lock().expect("DOM").propagation_stopped {
            break;
        }
    }
    if !stand.lock().expect("DOM").propagation_stopped {
        invoke_target(
            engine,
            stand,
            kiste,
            target,
            target,
            event_type,
            2,
            true,
            bubbles,
            x,
            y,
            fallback,
            &mut result,
        );
        if !stand.lock().expect("DOM").propagation_stopped {
            invoke_target(
                engine,
                stand,
                kiste,
                target,
                target,
                event_type,
                2,
                false,
                bubbles,
                x,
                y,
                fallback,
                &mut result,
            );
        }
    }
    if bubbles && !stand.lock().expect("DOM").propagation_stopped {
        for &host in &path {
            invoke_target(
                engine,
                stand,
                kiste,
                target,
                host,
                event_type,
                3,
                false,
                bubbles,
                x,
                y,
                fallback,
                &mut result,
            );
            if stand.lock().expect("DOM").propagation_stopped {
                break;
            }
        }
    }
    {
        let mut state = stand.lock().expect("DOM");
        state.passive_listener = false;
        state.event_cancelable = None;
        result.commands.append(&mut state.befehle);
    }
    result
        .commands
        .extend(kiste.lock().expect("Kiste").nimm_befehle());
    js::handler_wurzeln_erneuern(engine, stand, kiste);
    engine.browser_task_beenden();
    result
}

#[allow(clippy::too_many_arguments)]
fn invoke_target(
    engine: &mut Engine,
    stand: &dom::Geteilt,
    kiste: &js::Geteilt,
    target: u32,
    current: u32,
    kind: &str,
    phase: u8,
    capture: bool,
    bubbles: bool,
    x: f32,
    y: f32,
    fallback: &HashMap<(u32, String), Handler>,
    out: &mut DispatchResult,
) {
    let ids: Vec<ListenerId> = stand
        .lock()
        .expect("DOM")
        .event_listeners
        .snapshot(current, kind);
    for id in ids {
        let entry = {
            let mut state = stand.lock().expect("DOM");
            if state.immediate_stopped {
                break;
            }
            if !state
                .event_listeners
                .get(id)
                .is_some_and(|l| l.options.capture == capture)
            {
                continue;
            }
            state.event_listeners.prepare(id)
        };
        if let Some(entry) = entry {
            invoke(
                engine,
                stand,
                kiste,
                target,
                current,
                kind,
                phase,
                entry.options.passive,
                &entry.callback,
                false,
                bubbles,
                x,
                y,
                out,
            );
        }
    }
    if !capture && !stand.lock().expect("DOM").immediate_stopped {
        let callback = {
            let state = stand.lock().expect("DOM");
            state
                .horcher
                .get(&(current, kind.to_string()))
                .cloned()
                .or_else(|| {
                    state
                        .ord
                        .get(&current)
                        .and_then(|ord| fallback.get(&(*ord as u32, kind.to_string())).cloned())
                })
        };
        if let Some(callback) = callback {
            invoke(
                engine, stand, kiste, target, current, kind, phase, false, &callback, true,
                bubbles, x, y, out,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn invoke(
    engine: &mut Engine,
    stand: &dom::Geteilt,
    _kiste: &js::Geteilt,
    target: u32,
    current: u32,
    kind: &str,
    phase: u8,
    passive: bool,
    callback: &Handler,
    property: bool,
    bubbles: bool,
    x: f32,
    y: f32,
    out: &mut DispatchResult,
) {
    let (cancelable, prevented) = {
        let mut state = stand.lock().expect("DOM");
        state.passive_listener = passive;
        (state.event_cancelable, state.default_verhindert)
    };
    let event = Wert::Gegenstand(vec![
        ("type".into(), Wert::Text(kind.into())),
        ("target".into(), dom::host_wert(target)),
        ("currentTarget".into(), dom::host_wert(current)),
        ("eventPhase".into(), Wert::Zahl(phase as f64)),
        ("bubbles".into(), Wert::Wahr(bubbles)),
        ("cancelable".into(), Wert::Wahr(cancelable.unwrap_or(true))),
        ("defaultPrevented".into(), Wert::Wahr(prevented)),
        ("clientX".into(), Wert::Zahl(x as f64)),
        ("clientY".into(), Wert::Zahl(y as f64)),
        (
            "preventDefault".into(),
            Wert::Nativ(narsil_jsvm::PREVENT_DEFAULT.into()),
        ),
        (
            "stopPropagation".into(),
            Wert::Nativ("__stopPropagation".into()),
        ),
        (
            "stopImmediatePropagation".into(),
            Wert::Nativ("__stopImmediatePropagation".into()),
        ),
    ]);
    let before = engine.konsole().len();
    out.invoked = true;
    let result = match callback {
        Handler::Funktion(index) => {
            engine.handler_aufrufen_mit_this(*index, &[event], &dom::host_wert(current))
        }
        Handler::Quelltext(source) | Handler::Anweisung(source) => {
            if source.len() > js::MAX_SKRIPT_QUELLE {
                out.errors.push("Event handler source exceeds limit".into());
                return;
            }
            let _ = engine.host_global("__narsil_event", event);
            let _ = engine.host_global("__narsil_current_target", dom::host_wert(current));
            let source = if matches!(callback, Handler::Anweisung(_)) {
                format!(
                    "(function(event){{var ev=event;{source}}}).call(__narsil_current_target,__narsil_event)"
                )
            } else {
                format!("({source}).call(__narsil_current_target,__narsil_event)")
            };
            engine.ausfuehren(&source)
        }
    };
    match result {
        Ok(Wert::Wahr(false)) if property && cancelable.unwrap_or(true) => {
            stand.lock().expect("DOM").default_verhindert = true
        }
        Err(error) => out.errors.push(format!("{kind} handler: {error}")),
        _ => {}
    }
    let console = engine.konsole();
    out.console.extend(
        console
            .iter()
            .skip(if console.len() >= before { before } else { 0 })
            .cloned(),
    );
}
