//! Layer identifiers shared by footprints and boards. Schematic-side items
//! (symbols, sheets) use [`SCHEMATIC`], which carries no physical meaning.

use crate::document::LayerId;

pub const TOP_COPPER: LayerId = LayerId(0);
pub const BOTTOM_COPPER: LayerId = LayerId(1);
pub const TOP_SILK: LayerId = LayerId(2);
pub const BOTTOM_SILK: LayerId = LayerId(3);
pub const MECHANICAL_1: LayerId = LayerId(4);
pub const KEEP_OUT: LayerId = LayerId(5);
pub const TOP_MASK: LayerId = LayerId(6);
pub const BOTTOM_MASK: LayerId = LayerId(7);
pub const TOP_PASTE: LayerId = LayerId(8);
pub const BOTTOM_PASTE: LayerId = LayerId(9);
pub const TOP_COURTYARD: LayerId = LayerId(10);
pub const BOTTOM_COURTYARD: LayerId = LayerId(11);
pub const TOP_ASSEMBLY: LayerId = LayerId(12);
pub const BOTTOM_ASSEMBLY: LayerId = LayerId(13);
pub const BOARD_OUTLINE: LayerId = LayerId(14);
/// Through-hole pads: every copper layer plus both masks.
pub const MULTI_LAYER: LayerId = LayerId(15);
/// Inner copper layers are `INNER_COPPER_BASE + n` for n = 1..=30.
pub const INNER_COPPER_BASE: u16 = 32;
pub const SCHEMATIC: LayerId = LayerId(100);

pub fn inner_copper(n: u16) -> LayerId {
    LayerId(INNER_COPPER_BASE + n)
}

/// The layer on the opposite board side, used when flipping footprints.
pub fn flipped(layer: LayerId) -> LayerId {
    let pairs = [
        (TOP_COPPER, BOTTOM_COPPER),
        (TOP_SILK, BOTTOM_SILK),
        (TOP_MASK, BOTTOM_MASK),
        (TOP_PASTE, BOTTOM_PASTE),
        (TOP_COURTYARD, BOTTOM_COURTYARD),
        (TOP_ASSEMBLY, BOTTOM_ASSEMBLY),
    ];
    for (a, b) in pairs {
        if layer == a {
            return b;
        }
        if layer == b {
            return a;
        }
    }
    layer
}
