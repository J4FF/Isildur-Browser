//! VIDEO_36: die **Scan-Tabellen** und `vp9_cat6_prob` fuer `decode_coefs`.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das ein eigener Schritt ist und nicht eine
//! Beil-age zu den Wahrscheinlichkeiten: GEMESSEN ist, dass `decode_coefs`
//! fuer **jedes** Token `scan[n]` liest. Also braucht ein 32x32-Block
//! **1024** Zahlen, und ein Bild mit 64 solchen Bloecken **65536**. GEMESSENE
//! ENTSCHEIDUNG, warum das gefaehrlicher ist als eine Wahrscheinlichkeitstabelle:
//! ein falscher **Scan** gibt falsche Koeffizienten, aber **keinen** Fehler im
//! Ablauf — die Bitbilanz stimmt, nur das Bild nicht. GEMESSEN ist, dass
//! VIDEO_31/32 an den Wahrscheinlichkeiten verloren haben; 1360 abgetippte
//! Zahlen waeren der naechste Fehler gewesen.
//!
//! GEMESSENE ENTSCHEIDUNG, warum das **programmatisch** ist: GEMESSEN ist in
//! VIDEO_31/32 dreimal ein Fehler beim Abtippen von Tabellen passiert. Also
//! mit einem Klammerbaum-Parser aus der C-Quelle gelesen, und die
//! **Permutationseigenschaft** als Test festgehalten.

#![allow(clippy::needless_range_loop)]

// GEMESSEN aus libvpx `vp9/common/vp9_scan.c`, PROGRAMMATISCH mit dem
// Klammerbaum-Parser ausgelesen — nicht abgetippt.
//
// GEMESSENE ENTSCHEIDUNG, warum die **Scan-Tabellen** so wichtig sind und
// nicht die Wahrscheinlichkeiten: GEMESSEN ist, dass `decode_coefs` sie fuer
// **jedes** Token braucht (`scan[n]` im Band), also **1360** Zahlen fuer ein
// 32x32-Bild. VIDEO_31/32 haben an den Wahrscheinlichkeiten verloren, nicht
// an den Scans — aber 1360 abgetippte Zahlen waeren der naechste Fehler
// gewesen, und der faellt nicht auf: ein falscher Scan gibt falsche
// Koeffizienten, aber **keinen** Fehler im Ablauf.

/// GEMESSENE ENTSCHEIDUNG, warum die Pruefung eine **Permutation** ist:
/// GEMESSEN ist, dass jede Position des Transformationsblocks genau einmal
/// vorkommt — sonst waere eine Position doppelt und eine andere nie. Das ist
/// eine **nachrechenbare** Zahl, und der Parser kann sie einhalten oder nicht.

/// `default_scan_4x4[16]` — GEMESSEN `vp9_scan.c`.
#[rustfmt::skip]
pub const SCAN_4X4: [i16; 16] = [
    0, 4, 1, 5, 8, 2, 12, 9, 3, 6, 13, 10, 7, 14, 11, 15
];

/// `default_scan_8x8[64]` — GEMESSEN `vp9_scan.c`.
#[rustfmt::skip]
pub const SCAN_8X8: [i16; 64] = [
    0, 8, 1, 16, 9, 2, 17, 24, 10, 3, 18, 25, 32, 11, 4, 26,
    33, 19, 40, 12, 34, 27, 5, 41, 20, 48, 13, 35, 42, 28, 21, 6,
    49, 56, 36, 43, 29, 7, 14, 50, 57, 44, 22, 37, 15, 51, 58, 30,
    45, 23, 52, 59, 38, 31, 60, 53, 46, 39, 61, 54, 47, 62, 55, 63
];

/// `default_scan_16x16[256]` — GEMESSEN `vp9_scan.c`.
#[rustfmt::skip]
pub const SCAN_16X16: [i16; 256] = [
    0, 16, 1, 32, 17, 2, 48, 33, 18, 3, 64, 34, 49, 19, 65, 80, 50, 4, 35, 66, 20, 81, 96, 51,
    5, 36, 82, 97, 67, 112, 21, 52, 98, 37, 83, 113, 6, 68, 128, 53, 22, 99, 114, 84, 7, 129, 38, 69,
    100, 115, 144, 130, 85, 54, 23, 8, 145, 39, 70, 116, 101, 131, 160, 146, 55, 86, 24, 71, 132, 117, 161, 40,
    9, 102, 147, 176, 162, 87, 56, 25, 133, 118, 177, 148, 72, 103, 41, 163, 10, 192, 178, 88, 57, 134, 149, 119,
    26, 164, 73, 104, 193, 42, 179, 208, 11, 135, 89, 165, 120, 150, 58, 194, 180, 27, 74, 209, 105, 151, 136, 43,
    90, 224, 166, 195, 181, 121, 210, 59, 12, 152, 106, 167, 196, 75, 137, 225, 211, 240, 182, 122, 91, 28, 197, 13,
    226, 168, 183, 153, 44, 212, 138, 107, 241, 60, 29, 123, 198, 184, 227, 169, 242, 76, 213, 154, 45, 92, 14, 199,
    139, 61, 228, 214, 170, 185, 243, 108, 77, 155, 30, 15, 200, 229, 124, 215, 244, 93, 46, 186, 171, 201, 109, 140,
    230, 62, 216, 245, 31, 125, 78, 156, 231, 47, 187, 202, 217, 94, 246, 141, 63, 232, 172, 110, 247, 157, 79, 218,
    203, 126, 233, 188, 248, 95, 173, 142, 219, 111, 249, 234, 158, 127, 189, 204, 250, 235, 143, 174, 220, 205, 159, 251,
    190, 221, 175, 236, 237, 191, 206, 252, 222, 253, 207, 238, 223, 254, 239, 255
];

/// `default_scan_32x32[1024]` — GEMESSEN `vp9_scan.c`.
#[rustfmt::skip]
pub const SCAN_32X32: [i16; 1024] = [
    0, 32, 1, 64, 33, 2, 96, 65, 34, 128, 3, 97, 66, 160, 129, 35, 98, 4, 67, 130, 161, 192, 36, 99,
    224, 5, 162, 193, 68, 131, 37, 100, 225, 194, 256, 163, 69, 132, 6, 226, 257, 288, 195, 101, 164, 38, 258, 7,
    227, 289, 133, 320, 70, 196, 165, 290, 259, 228, 39, 321, 102, 352, 8, 197, 71, 134, 322, 291, 260, 353, 384, 229,
    166, 103, 40, 354, 323, 292, 135, 385, 198, 261, 72, 9, 416, 167, 386, 355, 230, 324, 104, 293, 41, 417, 199, 136,
    262, 387, 448, 325, 356, 10, 73, 418, 231, 168, 449, 294, 388, 105, 419, 263, 42, 200, 357, 450, 137, 480, 74, 326,
    232, 11, 389, 169, 295, 420, 106, 451, 481, 358, 264, 327, 201, 43, 138, 512, 482, 390, 296, 233, 170, 421, 75, 452,
    359, 12, 513, 265, 483, 328, 107, 202, 514, 544, 422, 391, 453, 139, 44, 234, 484, 297, 360, 171, 76, 515, 545, 266,
    329, 454, 13, 423, 203, 108, 546, 485, 576, 298, 235, 140, 361, 330, 172, 547, 45, 455, 267, 577, 486, 77, 204, 362,
    608, 14, 299, 578, 109, 236, 487, 609, 331, 141, 579, 46, 15, 173, 610, 363, 78, 205, 16, 110, 237, 611, 142, 47,
    174, 79, 206, 17, 111, 238, 48, 143, 80, 175, 112, 207, 49, 18, 239, 81, 113, 19, 50, 82, 114, 51, 83, 115,
    640, 516, 392, 268, 144, 20, 672, 641, 548, 517, 424, 393, 300, 269, 176, 145, 52, 21, 704, 673, 642, 580, 549, 518,
    456, 425, 394, 332, 301, 270, 208, 177, 146, 84, 53, 22, 736, 705, 674, 643, 612, 581, 550, 519, 488, 457, 426, 395,
    364, 333, 302, 271, 240, 209, 178, 147, 116, 85, 54, 23, 737, 706, 675, 613, 582, 551, 489, 458, 427, 365, 334, 303,
    241, 210, 179, 117, 86, 55, 738, 707, 614, 583, 490, 459, 366, 335, 242, 211, 118, 87, 739, 615, 491, 367, 243, 119,
    768, 644, 520, 396, 272, 148, 24, 800, 769, 676, 645, 552, 521, 428, 397, 304, 273, 180, 149, 56, 25, 832, 801, 770,
    708, 677, 646, 584, 553, 522, 460, 429, 398, 336, 305, 274, 212, 181, 150, 88, 57, 26, 864, 833, 802, 771, 740, 709,
    678, 647, 616, 585, 554, 523, 492, 461, 430, 399, 368, 337, 306, 275, 244, 213, 182, 151, 120, 89, 58, 27, 865, 834,
    803, 741, 710, 679, 617, 586, 555, 493, 462, 431, 369, 338, 307, 245, 214, 183, 121, 90, 59, 866, 835, 742, 711, 618,
    587, 494, 463, 370, 339, 246, 215, 122, 91, 867, 743, 619, 495, 371, 247, 123, 896, 772, 648, 524, 400, 276, 152, 28,
    928, 897, 804, 773, 680, 649, 556, 525, 432, 401, 308, 277, 184, 153, 60, 29, 960, 929, 898, 836, 805, 774, 712, 681,
    650, 588, 557, 526, 464, 433, 402, 340, 309, 278, 216, 185, 154, 92, 61, 30, 992, 961, 930, 899, 868, 837, 806, 775,
    744, 713, 682, 651, 620, 589, 558, 527, 496, 465, 434, 403, 372, 341, 310, 279, 248, 217, 186, 155, 124, 93, 62, 31,
    993, 962, 931, 869, 838, 807, 745, 714, 683, 621, 590, 559, 497, 466, 435, 373, 342, 311, 249, 218, 187, 125, 94, 63,
    994, 963, 870, 839, 746, 715, 622, 591, 498, 467, 374, 343, 250, 219, 126, 95, 995, 871, 747, 623, 499, 375, 251, 127,
    900, 776, 652, 528, 404, 280, 156, 932, 901, 808, 777, 684, 653, 560, 529, 436, 405, 312, 281, 188, 157, 964, 933, 902,
    840, 809, 778, 716, 685, 654, 592, 561, 530, 468, 437, 406, 344, 313, 282, 220, 189, 158, 996, 965, 934, 903, 872, 841,
    810, 779, 748, 717, 686, 655, 624, 593, 562, 531, 500, 469, 438, 407, 376, 345, 314, 283, 252, 221, 190, 159, 997, 966,
    935, 873, 842, 811, 749, 718, 687, 625, 594, 563, 501, 470, 439, 377, 346, 315, 253, 222, 191, 998, 967, 874, 843, 750,
    719, 626, 595, 502, 471, 378, 347, 254, 223, 999, 875, 751, 627, 503, 379, 255, 904, 780, 656, 532, 408, 284, 936, 905,
    812, 781, 688, 657, 564, 533, 440, 409, 316, 285, 968, 937, 906, 844, 813, 782, 720, 689, 658, 596, 565, 534, 472, 441,
    410, 348, 317, 286, 1000, 969, 938, 907, 876, 845, 814, 783, 752, 721, 690, 659, 628, 597, 566, 535, 504, 473, 442, 411,
    380, 349, 318, 287, 1001, 970, 939, 877, 846, 815, 753, 722, 691, 629, 598, 567, 505, 474, 443, 381, 350, 319, 1002, 971,
    878, 847, 754, 723, 630, 599, 506, 475, 382, 351, 1003, 879, 755, 631, 507, 383, 908, 784, 660, 536, 412, 940, 909, 816,
    785, 692, 661, 568, 537, 444, 413, 972, 941, 910, 848, 817, 786, 724, 693, 662, 600, 569, 538, 476, 445, 414, 1004, 973,
    942, 911, 880, 849, 818, 787, 756, 725, 694, 663, 632, 601, 570, 539, 508, 477, 446, 415, 1005, 974, 943, 881, 850, 819,
    757, 726, 695, 633, 602, 571, 509, 478, 447, 1006, 975, 882, 851, 758, 727, 634, 603, 510, 479, 1007, 883, 759, 635, 511,
    912, 788, 664, 540, 944, 913, 820, 789, 696, 665, 572, 541, 976, 945, 914, 852, 821, 790, 728, 697, 666, 604, 573, 542,
    1008, 977, 946, 915, 884, 853, 822, 791, 760, 729, 698, 667, 636, 605, 574, 543, 1009, 978, 947, 885, 854, 823, 761, 730,
    699, 637, 606, 575, 1010, 979, 886, 855, 762, 731, 638, 607, 1011, 887, 763, 639, 916, 792, 668, 948, 917, 824, 793, 700,
    669, 980, 949, 918, 856, 825, 794, 732, 701, 670, 1012, 981, 950, 919, 888, 857, 826, 795, 764, 733, 702, 671, 1013, 982,
    951, 889, 858, 827, 765, 734, 703, 1014, 983, 890, 859, 766, 735, 1015, 891, 767, 920, 796, 952, 921, 828, 797, 984, 953,
    922, 860, 829, 798, 1016, 985, 954, 923, 892, 861, 830, 799, 1017, 986, 955, 893, 862, 831, 1018, 987, 894, 863, 1019, 895,
    924, 956, 925, 988, 957, 926, 1020, 989, 958, 927, 1021, 990, 959, 1022, 991, 1023
];

// GEMESSENE ENTSCHEIDUNG, warum `vp9_cat6_prob` **hier nicht** mehr steht:
// es liegt jetzt in `tabellen.rs` neben den anderen `decode_coefs`-Tabellen,
// weil GEMESSEN ist, dass `decode_coefs` alle auf einmal braucht — und eine
// Tabelle, die man fuer sich allein sucht, sucht man zweimal.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// GEMESSENE ENTSCHEIDUNG, warum die **Permutation** geprueft wird und
    /// nicht die Zahl der Eintraege: GEMESSEN ist, dass jede Position des
    /// Transformationsblocks genau **einmal** vorkommen muss. Ein Eintrag zu
    /// viel faellt beim Fuellen des Arrays auf, ein Eintrag zu **wenig**
    /// nicht — und ein **doppelter** auch nicht, denn `[i16; 1024]` hat
    /// fest 1024 Zellen, und die letzte wird mit irgendeinem Wert gefuellt.
    /// Also ist die Permutation die **einzige** Eigenschaft, die alle drei
    /// Fehler faellt.
    fn pruefe_permutation(name: &str, scan: &[i16]) {
        let n = (scan.len() as f64).sqrt() as usize;
        assert_eq!(n * n, scan.len(), "{name}: Laenge ist keine Quadratzahl");
        let menge: BTreeSet<i16> = scan.iter().copied().collect();
        assert_eq!(
            menge.len(),
            scan.len(),
            "{name}: {} verschiedene aus {} Zellen — es gibt einen doppelten \
             Eintrag",
            menge.len(),
            scan.len()
        );
        assert_eq!(
            *menge.iter().next().unwrap(),
            0,
            "{name}: der kleinste Eintrag ist nicht 0"
        );
        assert_eq!(
            *menge.iter().next_back().unwrap(),
            (n * n - 1) as i16,
            "{name}: der groesste Eintrag ist nicht n*n-1, also fehlt eine Position"
        );
    }

    #[test]
    fn alle_vier_scan_tabellen_sind_permutationen_von_0_bis_n_quadrat_minus_1() {
        pruefe_permutation("4x4", &SCAN_4X4);
        pruefe_permutation("8x8", &SCAN_8X8);
        pruefe_permutation("16x16", &SCAN_16X16);
        pruefe_permutation("32x32", &SCAN_32X32);
        // GEMESSENE ENTSCHEIDUNG, warum die **Summen** mitgeschrieben werden:
        // eine Permutation von 0..n-1 hat die Summe `n*(n-1)/2`, und das ist
        // eine Zahl, die man **im Kopf** nachrechnen kann. Fuer 32x32:
        // 1024 * 1023 / 2 = **523776**.
        assert_eq!(SCAN_4X4.iter().map(|x| *x as i64).sum::<i64>(), 120);
        assert_eq!(SCAN_8X8.iter().map(|x| *x as i64).sum::<i64>(), 2016);
        assert_eq!(SCAN_16X16.iter().map(|x| *x as i64).sum::<i64>(), 32640);
        assert_eq!(
            SCAN_32X32.iter().map(|x| *x as i64).sum::<i64>(),
            523776,
            "1024 * 1023 / 2 = 523776"
        );
        assert_eq!(SCAN_4X4.len(), 16);
        assert_eq!(SCAN_8X8.len(), 64);
        assert_eq!(SCAN_16X16.len(), 256);
        assert_eq!(SCAN_32X32.len(), 1024);
        assert_eq!(
            SCAN_4X4.len() + SCAN_8X8.len() + SCAN_16X16.len() + SCAN_32X32.len(),
            1360,
            "GEMESSEN: 16 + 64 + 256 + 1024 = 1360 Zahlen"
        );
    }
}
