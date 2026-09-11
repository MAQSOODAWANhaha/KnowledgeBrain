use crate::{PdfTableCell, TableGrid};

pub fn validate_table_grid(grid: &TableGrid) -> Result<(), String> {
    if grid.row_count == 0 || grid.column_count == 0 {
        return Err("DocReader returned empty table grid".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut occupied = std::collections::BTreeMap::new();
    for cell in &grid.cells {
        if cell.row_span == 0
            || cell.col_span == 0
            || cell.row >= grid.row_count
            || cell.column >= grid.column_count
            || cell
                .row
                .checked_add(cell.row_span)
                .is_none_or(|end| end > grid.row_count)
            || cell
                .column
                .checked_add(cell.col_span)
                .is_none_or(|end| end > grid.column_count)
            || !seen.insert((cell.row, cell.column))
        {
            return Err("DocReader returned invalid table grid cell".into());
        }
        for row in cell.row..cell.row + cell.row_span {
            for column in cell.column..cell.column + cell.col_span {
                let owner = (cell.row, cell.column);
                if occupied.insert((row, column), owner).is_some() {
                    return Err("DocReader returned overlapping table grid spans".into());
                }
                if (row, column) != owner && seen.contains(&(row, column)) {
                    return Err("DocReader returned covered table grid slot".into());
                }
            }
        }
    }
    let expected = (grid.row_count as u64)
        .checked_mul(grid.column_count as u64)
        .ok_or_else(|| "DocReader returned overflowing table grid".to_string())?;
    if occupied.len() as u64 != expected {
        return Err("DocReader returned table grid that does not tile".into());
    }
    if let Some(widths) = &grid.widths_mm
        && (widths.len() != grid.column_count as usize
            || widths
                .iter()
                .any(|width| !width.is_finite() || *width <= 0.0))
    {
        return Err("DocReader returned invalid table grid widths".into());
    }
    Ok(())
}

pub fn from_proto_grid(value: crate::proto::TableGrid) -> Result<TableGrid, String> {
    let widths_mm = if value.widths_mm.is_empty() {
        None
    } else {
        Some(value.widths_mm)
    };
    let mut cells = Vec::with_capacity(value.cells.len());
    for cell in value.cells {
        cells.push(PdfTableCell {
            row: cell.row,
            column: cell.column,
            row_span: cell.row_span,
            col_span: cell.col_span,
            text: cell.text,
        });
    }
    cells.sort_by_key(|cell| (cell.row, cell.column));
    let grid = TableGrid {
        row_count: value.row_count,
        column_count: value.column_count,
        cells,
        widths_mm,
    };
    validate_table_grid(&grid)?;
    Ok(grid)
}

pub fn grid_cell_is_anchor(grid: &TableGrid, row: u32, column: u32) -> bool {
    grid.cells.iter().any(|cell| {
        cell.row == row
            && cell.column == column
            && !grid.cells.iter().any(|other| {
                (other.row, other.column) != (row, column)
                    && row >= other.row
                    && column >= other.column
                    && row < other.row + other.row_span
                    && column < other.column + other.col_span
            })
    })
}
