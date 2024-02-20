use super::{area::Area, table_renderer::{BorderType, BorderLineStyle, Rect, Border}, range::Range};

// #[derive(Debug, Clone)]
// pub struct Border {
//     reference: String,
//     border_type: BorderType,
//     style: BorderLineStyle, 
//     color: String
// }

pub fn border_ranges(area: &Area, border: &Border, area_merges: Vec<Range>) -> Vec<(Range, Rect, BorderType)> {
    let border_ref = border.reference.clone();
    let border_type = border.border_type.clone();
    let border_range = Range::with(&border_ref);
    let mut intersect_merges = area_merges.iter().filter(|r| r.intersects(&border_range)).collect::<Vec<&Range>>();

    let mut ret: Vec<(Range, Rect, BorderType)> = vec![]; 

    if border_range.intersects(&area.range) || intersect_merges.len() > 0 {
        if intersect_merges.len() <= 0 {
            ret.push((border_range.clone(), area.rect(&border_range), border_type.clone()));
        } else {
            for merge in intersect_merges {
                if border_range.within(merge) {
                    if border_range.start_row == merge.start_row 
                        && border_range.start_col == merge.start_col
                        && border_type != BorderType::Inside
                        && border_type != BorderType::Horizontal
                        && border_type != BorderType::Vertical {
                        ret.push((
                            merge.clone(), 
                            area.rect(merge), 
                            if border_type == BorderType::All { BorderType::Outside} else { border_type }))
                    }
                } else if border_type == BorderType::Outside
                    || border_type == BorderType::Left
                    || border_type == BorderType::Top
                    || border_type == BorderType::Right
                    || border_type == BorderType::Bottom {
                    ret.push((
                        border_range.clone(),
                        area.rect(&border_range),
                        border_type.clone()
                    ));
                    break;
                } else {
                    // TODO: Implement this
                }
            }
        }
    }
    ret
}