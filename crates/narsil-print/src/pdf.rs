use narsil_image::Bild;
use narsil_image::deflate::komprimiere;
use narsil_image::pruefsummen::adler32;

pub struct PdfWriter {
    xref: Vec<usize>,
    out: Vec<u8>,
    pages: Vec<usize>,
}

impl PdfWriter {
    pub fn new() -> Self {
        // Pre-allocate 1 (Catalog) and 2 (Pages)
        Self {
            xref: vec![0, 0, 0], 
            out: b"%PDF-1.4\n".to_vec(),
            pages: Vec::new(),
        }
    }

    fn add_object(&mut self, data: &[u8]) -> usize {
        let id = self.xref.len();
        self.xref.push(self.out.len());
        self.out.extend_from_slice(format!("{} 0 obj\n", id).as_bytes());
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendobj\n");
        id
    }

    fn write_object_at(&mut self, id: usize, data: &[u8]) {
        self.xref[id] = self.out.len();
        self.out.extend_from_slice(format!("{} 0 obj\n", id).as_bytes());
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    pub fn add_page(&mut self, image: Option<&Bild>, text: &str) {
        let font_obj = b"<< /Type /Font /Subtype /Type1 /Name /F1 /BaseFont /Helvetica >>".to_vec();
        let font_id = self.add_object(&font_obj);
        
        let mut resources = format!("<< /Font << /F1 {} 0 R >> ", font_id);
        let mut contents_str = String::new();
        contents_str.push_str("BT\n/F1 12 Tf\n50 750 Td\n");
        contents_str.push_str(&format!("({}) Tj\nET\n", text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")));
        
        if let Some(image) = image {
            let mut rgb = Vec::with_capacity((image.breite * image.hoehe * 3) as usize);
            for chunk in image.rgba.chunks_exact(4) {
                rgb.push(chunk[0]);
                rgb.push(chunk[1]);
                rgb.push(chunk[2]);
            }

            let compressed = komprimiere(&rgb);
            let mut zlib = vec![0x78, 0x9c]; // Default compression
            zlib.extend_from_slice(&compressed);
            let adler = adler32(&rgb);
            zlib.extend_from_slice(&adler.to_be_bytes());
            
            let image_dict = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                image.breite, image.hoehe, zlib.len()
            );
            let mut image_obj = image_dict.into_bytes();
            image_obj.extend_from_slice(&zlib);
            image_obj.extend_from_slice(b"\nendstream");
            let img_id = self.add_object(&image_obj);
            
            resources.push_str(&format!("/XObject << /Im1 {} 0 R >> ", img_id));
            
            contents_str.push_str(&format!("q\n300 0 0 300 50 400 cm\n/Im1 Do\nQ\n"));
        }
        resources.push_str(">>");

        let contents_obj = format!("<< /Length {} >>\nstream\n{}\nendstream", contents_str.len(), contents_str);
        let contents_id = self.add_object(contents_obj.as_bytes());

        let page_dict = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents {} 0 R /Resources {} >>",
            contents_id, resources
        );
        let page_id = self.add_object(page_dict.as_bytes());
        self.pages.push(page_id);
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cat_dict = format!("<< /Type /Catalog /Pages 2 0 R >>");
        self.write_object_at(1, cat_dict.as_bytes());
        
        let kids: Vec<String> = self.pages.iter().map(|id| format!("{} 0 R", id)).collect();
        let pages_dict = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), self.pages.len());
        self.write_object_at(2, pages_dict.as_bytes());
        
        let xref_pos = self.out.len();
        self.out.extend_from_slice(format!("xref\n0 {}\n", self.xref.len()).as_bytes());
        for (i, &pos) in self.xref.iter().enumerate() {
            if i == 0 {
                self.out.extend_from_slice(b"0000000000 65535 f \n");
            } else {
                self.out.extend_from_slice(format!("{:010} 00000 n \n", pos).as_bytes());
            }
        }

        self.out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", self.xref.len(), xref_pos).as_bytes());

        self.out
    }
}
