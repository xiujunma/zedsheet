//! Mobile view-only responsive helpers (Phase 7).
//!
//! Pure decision functions — viewport-width → layout class, view-only
//! → formula bar visibility, etc. Host-testable: no DOM / JS needed.

/// Which layout bucket the current viewport falls into.
/// Buckets are decided by `breakpoint_class`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Desktop,
    Tablet,
    PhoneLarge,
    Phone,
}

/// Classify the viewport width into a layout bucket. The
/// thresholds match the spec's breakpoint table (1024 / 768 / 480).
pub fn breakpoint_class(width: u32) -> Breakpoint {
    if width >= 1024 {
        Breakpoint::Desktop
    } else if width >= 768 {
        Breakpoint::Tablet
    } else if width >= 480 {
        Breakpoint::PhoneLarge
    } else {
        Breakpoint::Phone
    }
}

/// True when the formula bar should render. The bar is hidden
/// at < 768 px (saving vertical space on phones + tablets), and
/// always hidden in view-only mode (the cell editor is suppressed,
/// so the bar would be dead UI).
pub fn should_show_formula_bar(width: u32, view_only: bool) -> bool {
    !view_only && width >= 768
}

/// The toolbar buttons visible at the given width. Desktop shows
/// the full set; tablet strips the text labels (icon-only via
/// the existing dropdown component); phone collapses further to
/// only essential actions. Phase 7 ships the data layer here;
/// the actual CSS hides the rest at each breakpoint.
pub fn toolbar_button_subset(width: u32) -> &'static [&'static str] {
    // The literal action ids mirror what's in
    // `src/zedsheet/context_menu.rs` / `src/zedsheet/toolbar.rs`.
    // Keep this list in sync if new toolbar actions are added.
    const DESKTOP: &[&str] = &[
        "undo", "redo", "print", "font-bold", "font-italic",
        "underline", "strike", "color", "bgcolor", "merge",
        "borders", "halign", "valign", "textwrap", "freeze",
        "autofilter", "formula",
    ];
    const TABLET: &[&str] = &[
        "undo", "redo", "print", "font-bold", "font-italic",
        "underline", "freeze", "autofilter", "formula",
    ];
    const PHONE_LARGE: &[&str] = &[
        "undo", "redo", "print", "freeze", "autofilter", "formula",
    ];
    const PHONE: &[&str] = &[
        "print", "autofilter", "formula",
    ];
    match breakpoint_class(width) {
        Breakpoint::Desktop => DESKTOP,
        Breakpoint::Tablet => TABLET,
        Breakpoint::PhoneLarge => PHONE_LARGE,
        Breakpoint::Phone => PHONE,
    }
}

/// True when the given toolbar / context-menu action should be
/// suppressed in view-only mode. Used by the events.rs handlers
/// to short-circuit before the existing mutation paths.
pub fn view_only_blocks(action: &str) -> bool {
    // Single source of truth for "what's allowed in view-only".
    // The non-listed actions (selection, scroll, zoom, sheet tabs)
    // remain enabled.
    matches!(
        action,
        "edit"            // double-click → cell editor
        | "copy"
        | "cut"
        | "paste"
        | "paste-values"
        | "paste-formulas"
        | "paste-formats"
        | "paste-transpose"
        | "paste-link"
        | "insert-row"
        | "insert-col"
        | "delete-row"
        | "delete-col"
        | "insert-cells-down"
        | "insert-cells-right"
        | "delete-cells-up"
        | "delete-cells-left"
        | "clear"
        | "font-bold" | "font-italic" | "underline" | "strike"
        | "color" | "bgcolor"
        | "merge" | "borders"
        | "halign" | "valign" | "textwrap" | "rotate-0" | "rotate-45"
        | "rotate-90" | "rotate--45" | "shrink-toggle"
        | "indent-inc" | "indent-dec"
        | "lock-unlock"
        | "validation" | "condfmt" | "chart" | "image" | "sparkline"
        | "pivot" | "slicer" | "protect" | "refresh-pivot"
        | "group-rows" | "ungroup-rows" | "group-cols" | "ungroup-cols"
        | "subtotal" | "sort-range"
        | "format-table" | "format-as-rich" | "format-as-plain"
        | "table-totals" | "table-to-range"
        | "page-break-row" | "page-break-col" | "page-break-remove"
        | "shape"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_class_thresholds() {
        // Pin every threshold + the two endpoints so a future
        // tweak is a deliberate change.
        assert_eq!(breakpoint_class(1440), Breakpoint::Desktop);
        assert_eq!(breakpoint_class(1024), Breakpoint::Desktop); // boundary
        assert_eq!(breakpoint_class(1023), Breakpoint::Tablet);
        assert_eq!(breakpoint_class(768), Breakpoint::Tablet);  // boundary
        assert_eq!(breakpoint_class(767), Breakpoint::PhoneLarge);
        assert_eq!(breakpoint_class(480), Breakpoint::PhoneLarge); // boundary
        assert_eq!(breakpoint_class(479), Breakpoint::Phone);
        assert_eq!(breakpoint_class(360), Breakpoint::Phone);
    }

    #[test]
    fn formula_bar_hidden_on_phones_and_in_view_only() {
        assert!(should_show_formula_bar(1440, false));
        assert!(should_show_formula_bar(768, false));
        assert!(!should_show_formula_bar(767, false));
        assert!(!should_show_formula_bar(360, false));
        // View-only always hides the bar, even on desktop.
        assert!(!should_show_formula_bar(1440, true));
        assert!(!should_show_formula_bar(768, true));
    }

    #[test]
    fn toolbar_subset_shrinks_with_viewport() {
        // Desktop has the full set; phone has only the essentials.
        let desktop = toolbar_button_subset(1440);
        let tablet = toolbar_button_subset(800);
        let phone_large = toolbar_button_subset(600);
        let phone = toolbar_button_subset(360);
        assert!(desktop.len() > tablet.len());
        assert!(tablet.len() > phone_large.len());
        assert!(phone_large.len() > phone.len());
        // Phone shows print + sheet switcher + formula picker — the
        // survival kit, nothing else.
        assert!(phone.contains(&"print"));
        assert!(phone.contains(&"autofilter"));
        assert!(phone.contains(&"formula"));
    }

    #[test]
    fn view_only_blocks_editing_actions_only() {
        // Blocked: every action that mutates data.
        for a in [
            "edit", "copy", "cut", "paste",
            "insert-row", "delete-col",
            "font-bold", "color", "merge",
            "condfmt", "chart", "image", "shape",
        ] {
            assert!(view_only_blocks(a), "{a} should be blocked in view-only");
        }
        // Not blocked: navigation + read-only actions.
        for a in ["print", "autofilter", "formula"] {
            assert!(!view_only_blocks(a), "{a} should remain enabled in view-only");
        }
        // Unknown action: not blocked (default = allow).
        assert!(!view_only_blocks("does-not-exist"));
    }
}
