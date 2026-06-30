use super::{
    area::Area,
    range::Range,
    table_renderer::{Border, BorderLineStyle, BorderType, Rect},
};

pub fn border_ranges(
    area: &Area,
    border: &Border,
    area_merges: Vec<Range>,
) -> Vec<(Range, Rect, BorderType)> {
    let border_ref = border.reference.clone();
    let border_type = border.border_type;
    let border_range = Range::with(&border_ref);
    let intersect_merges = area_merges
        .iter()
        .filter(|r| r.intersects(&border_range))
        .cloned()
        .collect::<Vec<Range>>();

    let mut ret: Vec<(Range, Rect, BorderType)> = vec![];

    if border_range.intersects(&area.range) || !intersect_merges.is_empty() {
        if intersect_merges.len() <= 0 {
            ret.push((border_range, area.rect(&border_range), border_type));
        } else {
            for merge in &intersect_merges {
                if border_range.within(merge) {
                    if border_range.start_row == merge.start_row
                        && border_range.start_col == merge.start_col
                        && border_type != BorderType::Inside
                        && border_type != BorderType::Horizontal
                        && border_type != BorderType::Vertical
                    {
                        ret.push((
                            *merge,
                            area.rect(merge),
                            if border_type == BorderType::All {
                                BorderType::Outside
                            } else {
                                border_type
                            },
                        ))
                    }
                } else if border_type == BorderType::Outside
                    || border_type == BorderType::Left
                    || border_type == BorderType::Top
                    || border_type == BorderType::Right
                    || border_type == BorderType::Bottom
                {
                    ret.push((border_range, area.rect(&border_range), border_type));
                    break;
                } else {
                    let imerges = intersect_merges
                        .iter()
                        .filter(|it| *(*it) != *merge)
                        .cloned()
                        .collect::<Vec<Range>>();
                    border_range.difference(merge).iter().for_each(|it| {
                        if it.intersects(&area.range) {
                            let border_rect = area.rect(it);
                            let border = Border {
                                reference: it.to_string(),
                                border_type,
                                border_line: BorderLineStyle::Thin,
                                color: String::from(""),
                            };
                            let border_ranges = border_ranges(area, &border, imerges.clone());
                            border_ranges.into_iter().for_each(|range| ret.push(range));

                            if border_type == BorderType::Inside
                                || border_type == BorderType::Horizontal
                            {
                                if it.start_row < merge.start_row && it.end_row < merge.start_row {
                                    // top
                                    ret.push((*it, border_rect, BorderType::Bottom));
                                } else if it.start_row > merge.start_row
                                    && it.end_row > merge.start_row
                                {
                                    // bottom
                                    ret.push((*it, border_rect, BorderType::Top));
                                }
                            }

                            if border_type == BorderType::Inside
                                || border_type == BorderType::Vertical
                            {
                                if it.start_col < merge.start_col && it.end_col < merge.start_col {
                                    // left
                                    ret.push((*it, border_rect, BorderType::Right));
                                }
                                if it.start_col > merge.start_col && it.end_col > merge.start_col {
                                    // right
                                    ret.push((*it, border_rect, BorderType::Left));
                                }
                            }
                        }
                    });
                    if border_type == BorderType::All {
                        let border_rect = area.rect(merge);
                        if border_range.start_row == merge.start_row {
                            ret.push((*merge, border_rect, BorderType::Top));
                        }

                        if border_range.end_row == merge.end_row {
                            ret.push((*merge, border_rect, BorderType::Bottom));
                        }

                        if border_range.start_col == merge.start_col {
                            ret.push((*merge, border_rect, BorderType::Left));
                        }

                        if border_range.end_col == merge.end_col {
                            ret.push((*merge, border_rect, BorderType::Right));
                        }
                    }
                    break;
                }
            }
        }
    }
    ret
}
