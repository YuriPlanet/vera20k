//! Phobos SuperWeaponSidebar geometry.
//!
//! Source: `SWSidebarClass::SortButtons` and `AddColumn`
//! (`src/Ext/Sidebar/SWSidebar/SWSidebarClass.cpp` L11-L30 and L157-L251),
//! `SWColumnClass::Draw`, and `ToggleSWButtonClass::UpdatePosition`.
//! Coordinates are sidebar pixels; the caller applies the sidebar size scale.

use crate::rules::phobos_sw_sidebar::SuperWeaponSidebarUi;
use crate::ui::sidebar::Rect;

const CAMEO_WIDTH: i32 = 60;
const CAMEO_HEIGHT: i32 = 48;

#[derive(Debug, Clone, PartialEq)]
pub struct SwSidebarLayoutEntry {
    pub type_id: String,
    pub rect: Rect,
    /// Zero-based column. The first column owns the ten positional hotkeys.
    pub column: usize,
    pub column_button: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwSidebarColumn {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub first_button: usize,
    pub button_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SwSidebarLayout {
    pub columns: Vec<SwSidebarColumn>,
    pub entries: Vec<SwSidebarLayoutEntry>,
    pub toggle: Option<Rect>,
}

/// `SWSidebarClass::AddColumn` capacity: each column holds `firstColumn -
/// columnIndex` buttons when pyramidal, otherwise `firstColumn`, and creation
/// stops at `MaxColumns` or when that capacity reaches zero. `SortButtons`
/// places only those buttons; the rest keep their right-sidebar cameo.
fn accepted_button_count(ui: SuperWeaponSidebarUi, first_column: i32, available: usize) -> usize {
    let mut accepted = 0usize;
    let mut column = 0i32;
    while column < ui.max_columns && accepted < available {
        let capacity = if ui.pyramid {
            first_column - column
        } else {
            first_column
        };
        if capacity <= 0 {
            break;
        }
        accepted += capacity as usize;
        column += 1;
    }
    accepted.min(available)
}

/// `SortButtons` layout. `top_height`/`bottom_height` are the loaded
/// TopPCX/BottomPCX heights; `toggle_size` is the loaded On/Off button size.
pub fn compute_layout(
    ui: SuperWeaponSidebarUi,
    type_ids: &[String],
    screen_height: i32,
    top_height: i32,
    bottom_height: i32,
    toggle_size: Option<[i32; 2]>,
    expanded: bool,
) -> SwSidebarLayout {
    let mut layout = SwSidebarLayout::default();
    let first_column = ui.column_capacity(screen_height);
    if !ui.enabled || first_column <= 0 || ui.max_columns <= 0 {
        return layout;
    }
    let count = accepted_button_count(ui, first_column, type_ids.len());
    if count == 0 {
        // `ToggleSWButtonClass::Draw` refuses while there are no columns.
        return layout;
    }

    let pitch = ui.cameo_height.max(48);
    let half_gap = (pitch - CAMEO_HEIGHT) / 2;
    let mut location_y = (screen_height - count.min(first_column as usize) as i32 * pitch) / 2;
    let mut location_x = ui.left_offset;
    let mut location_y_pos = location_y + half_gap;
    let mut row = 0i32;
    let mut column = 0usize;

    for (button_index, type_id) in type_ids.iter().take(count).enumerate() {
        let capacity = if ui.pyramid {
            first_column - column as i32
        } else {
            first_column
        };
        if capacity <= 0 || column as i32 >= ui.max_columns {
            break;
        }
        if row == 0 {
            layout.columns.push(SwSidebarColumn {
                x: location_x - ui.left_offset,
                y: location_y - top_height,
                width: CAMEO_WIDTH + ui.interval,
                height: 0,
                first_button: button_index,
                button_count: 0,
            });
        }
        layout.entries.push(SwSidebarLayoutEntry {
            type_id: type_id.clone(),
            rect: Rect {
                x: location_x as f32,
                y: location_y_pos as f32,
                w: CAMEO_WIDTH as f32,
                h: CAMEO_HEIGHT as f32,
            },
            column,
            column_button: row as usize,
        });
        if let Some(current) = layout.columns.last_mut() {
            current.button_count += 1;
        }
        row += 1;
        if row >= capacity {
            row = 0;
            column += 1;
            if ui.pyramid {
                location_y += pitch / 2;
            }
            location_x += CAMEO_WIDTH + ui.interval;
            location_y_pos = location_y + half_gap;
        } else {
            location_y_pos += pitch;
        }
    }

    for column_layout in &mut layout.columns {
        column_layout.height =
            column_layout.button_count as i32 * pitch + top_height + bottom_height;
    }

    if let (Some([w, h]), Some(last)) = (toggle_size, layout.columns.last()) {
        let x = if expanded { last.x + last.width } else { 0 };
        let y = last.y + (last.height - h) / 2;
        layout.toggle = Some(Rect {
            x: x as f32,
            y: y as f32,
            w: w as f32,
            h: h as f32,
        });
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("SW{i}")).collect()
    }

    #[test]
    fn pyramid_shortens_each_column_and_centers_the_first() {
        let mut ui = SuperWeaponSidebarUi {
            enabled: true,
            ..SuperWeaponSidebarUi::default()
        };
        ui.max = 4;
        let layout = compute_layout(ui, &ids(7), 600, 10, 12, None, true);
        assert_eq!(layout.columns.len(), 2);
        assert_eq!(layout.columns[0].button_count, 4);
        assert_eq!(layout.columns[1].button_count, 3);
        assert_eq!(layout.columns[0].y, layout_center(600, 4) - 10);
        // `SortButtons` bumps the column base by pitch/2 on a column switch
        // instead of advancing it per row, so the next column starts 24 px
        // below the first and 60 px to the right.
        assert_eq!(layout.columns[0].x, 0);
        assert_eq!(layout.columns[1].x, 60);
        assert_eq!(layout.columns[1].y, layout_center(600, 4) + 24 - 10);
        assert_eq!(layout.entries[4].rect.y, layout.entries[0].rect.y + 24.0);
        assert_eq!(layout.columns[0].height, 4 * 48 + 10 + 12);
        assert_eq!(layout.columns[1].height, 3 * 48 + 10 + 12);
    }

    fn layout_center(screen_h: i32, rows: i32) -> i32 {
        (screen_h - rows * 48) / 2
    }

    #[test]
    fn rectangular_columns_use_the_same_capacity_and_overflow_is_dropped() {
        let mut ui = SuperWeaponSidebarUi {
            enabled: true,
            pyramid: false,
            max: 3,
            max_columns: 2,
            ..SuperWeaponSidebarUi::default()
        };
        ui.interval = 5;
        let layout = compute_layout(ui, &ids(8), 600, 0, 0, None, true);
        assert_eq!(layout.columns.len(), 2);
        assert_eq!(
            layout.entries.len(),
            6,
            "overflow stays on the right sidebar"
        );
        assert_eq!(layout.columns[1].x, 65);
        assert_eq!(layout.entries[3].rect.x, 65.0);
        assert_eq!(layout.entries[3].rect.y, layout.entries[0].rect.y);
    }

    #[test]
    fn max_columns_zero_and_tiny_screens_draw_nothing() {
        let mut ui = SuperWeaponSidebarUi {
            enabled: true,
            ..SuperWeaponSidebarUi::default()
        };
        ui.max_columns = 0;
        assert!(
            compute_layout(ui, &ids(3), 600, 0, 0, None, true)
                .entries
                .is_empty()
        );
        ui.max_columns = i32::MAX;
        assert!(
            compute_layout(ui, &ids(3), 80, 0, 0, None, true)
                .entries
                .is_empty()
        );
    }

    #[test]
    fn toggle_follows_the_last_column_or_the_screen_center_without_one() {
        let mut ui = SuperWeaponSidebarUi {
            enabled: true,
            ..SuperWeaponSidebarUi::default()
        };
        ui.max = 2;
        let layout = compute_layout(ui, &ids(2), 600, 10, 10, Some([30, 20]), true);
        let toggle = layout.toggle.unwrap();
        assert_eq!(toggle.x, 60.0);
        assert_eq!(
            toggle.y,
            (layout.columns[0].y + (layout.columns[0].height - 20) / 2) as f32
        );

        let collapsed = compute_layout(ui, &ids(2), 600, 10, 10, Some([30, 20]), false);
        assert_eq!(collapsed.toggle.unwrap().x, 0.0);

        let empty = compute_layout(ui, &ids(0), 600, 0, 0, Some([30, 20]), true);
        assert!(empty.toggle.is_none());
    }
}
