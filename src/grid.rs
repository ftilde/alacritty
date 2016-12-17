// Copyright 2016 Joe Wilm, The Alacritty Project Contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A generic 2d grid implementation optimized for use in a terminal.
//!
//! The current implementation uses a vector of vectors to store cell data.
//! Reimplementing the store as a single contiguous vector may be desirable in
//! the future. Rotation and indexing would need to be reconsidered at that
//! time; rotation currently reorganize Vecs in the lines Vec, and indexing with
//! ranges is currently supported.

use std::borrow::ToOwned;
use std::cmp::Ordering;
use std::iter::IntoIterator;
use std::ops::{Deref, DerefMut, Range, RangeTo, RangeFrom, RangeFull, Index, IndexMut};
use std::slice::{self, Iter, IterMut};

use index::{self, Cursor};

/// Represents the terminal display contents
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Grid<T> {
    /// Lines in the grid. Each row holds a list of cells corresponding to the
    /// columns in that row.
    raw: Vec<Row<T>>,

    /// Number of columns
    cols: index::Column,

    /// Number of lines.
    ///
    /// Invariant: lines is equivalent to raw.len()
    lines: index::Line,
}

impl<T: Clone> Grid<T> {
    pub fn new(lines: index::Line, cols: index::Column, template: &T) -> Grid<T> {
        let mut raw = Vec::with_capacity(*lines);
        for _ in index::Line(0)..lines {
            raw.push(Row::new(cols, template));
        }

        Grid {
            raw: raw,
            cols: cols,
            lines: lines,
        }
    }

    pub fn resize(&mut self, lines: index::Line, cols: index::Column, template: &T) {
        // Check that there's actually work to do and return early if not
        if lines == self.lines && cols == self.cols {
            return;
        }

        match self.lines.cmp(&lines) {
            Ordering::Less => self.grow_lines(lines, template),
            Ordering::Greater => self.shrink_lines(lines),
            Ordering::Equal => (),
        }

        match self.cols.cmp(&cols) {
            Ordering::Less => self.grow_cols(cols, template),
            Ordering::Greater => self.shrink_cols(cols),
            Ordering::Equal => (),
        }
    }

    fn grow_lines(&mut self, lines: index::Line, template: &T) {
        for _ in self.num_lines()..lines {
            self.raw.push(Row::new(self.cols, template));
        }

        self.lines = lines;
    }

    fn grow_cols(&mut self, cols: index::Column, template: &T) {
        for row in self.lines_mut() {
            row.grow(cols, template);
        }

        self.cols = cols;
    }

}

impl<T> Grid<T> {
    #[inline]
    pub fn lines(&self) -> Iter<Row<T>> {
        self.raw.iter()
    }

    #[inline]
    pub fn lines_mut(&mut self) -> IterMut<Row<T>> {
        self.raw.iter_mut()
    }

    #[inline]
    pub fn num_lines(&self) -> index::Line {
        self.lines
    }

    #[inline]
    pub fn num_cols(&self) -> index::Column {
        self.cols
    }

    #[inline]
    pub fn scroll_down(&mut self, region: Range<index::Line>, positions: index::Line) {
        for line in region.rev() {
            let src = line;
            let dst = line - positions;
            self.swap_lines(src, dst);
        }
    }

    #[inline]
    pub fn scroll_up(&mut self, region: Range<index::Line>, positions: index::Line) {
        for line in region {
            let src = line;
            let dst = line + positions;
            self.swap_lines(src, dst);
        }
    }

    #[inline]
    pub fn contains(&self, cursor: &Cursor) -> bool {
        self.lines > cursor.line && self.cols > cursor.col
    }

    /// Swap two lines in the grid
    ///
    /// This could have used slice::swap internally, but we are able to have
    /// better error messages by doing the bounds checking ourselves.
    #[inline]
    pub fn swap_lines(&mut self, src: index::Line, dst: index::Line) {
        use std::intrinsics::unlikely;

        unsafe {
            // check that src/dst are in bounds. Since index::Line newtypes usize,
            // we can assume values are positive.
            if unlikely(src >= self.lines) {
                panic!("swap_lines src out of bounds; len={}, src={}", self.raw.len(), src);
            }

            if unlikely(dst >= self.lines) {
                panic!("swap_lines dst out of bounds; len={}, dst={}", self.raw.len(), dst);
            }

            let src: *mut _ = self.raw.get_unchecked_mut(src.0);
            let dst: *mut _ = self.raw.get_unchecked_mut(dst.0);

            ::std::ptr::swap(src, dst);
        }
    }

    #[inline]
    pub fn clear<F: Fn(&mut T)>(&mut self, func: F) {
        let region = index::Line(0)..self.num_lines();
        self.clear_region(region, func);
    }

    fn shrink_lines(&mut self, lines: index::Line) {
        while index::Line(self.raw.len()) != lines {
            self.raw.pop();
        }

        self.lines = lines;
    }

    fn shrink_cols(&mut self, cols: index::Column) {
        for row in self.lines_mut() {
            row.shrink(cols);
        }

        self.cols = cols;
    }
}

impl<T> Index<index::Line> for Grid<T> {
    type Output = Row<T>;

    #[inline]
    fn index(&self, index: index::Line) -> &Row<T> {
        &self.raw[index.0]
    }
}

impl<T> IndexMut<index::Line> for Grid<T> {
    #[inline]
    fn index_mut(&mut self, index: index::Line) -> &mut Row<T> {
        &mut self.raw[index.0]
    }
}

impl<'cursor, T> Index<&'cursor Cursor> for Grid<T> {
    type Output = T;

    #[inline]
    fn index<'a>(&'a self, cursor: &Cursor) -> &'a T {
        &self.raw[cursor.line.0][cursor.col]
    }
}

impl<'cursor, T> IndexMut<&'cursor Cursor> for Grid<T> {
    #[inline]
    fn index_mut<'a, 'b>(&'a mut self, cursor: &'b Cursor) -> &'a mut T {
        &mut self.raw[cursor.line.0][cursor.col]
    }
}

/// A row in the grid
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Row<T>(Vec<T>);

impl<T: Clone> Row<T> {
    pub fn new(columns: index::Column, template: &T) -> Row<T> {
        Row(vec![template.to_owned(); *columns])
    }

    pub fn grow(&mut self, cols: index::Column, template: &T) {
        while self.len() != *cols {
            self.push(template.to_owned());
        }
    }
}

impl<T> Row<T> {
    pub fn shrink(&mut self, cols: index::Column) {
        while self.len() != *cols {
            self.pop();
        }
    }

    #[inline]
    pub fn cells(&self) -> Iter<T> {
        self.0.iter()
    }

    #[inline]
    pub fn cells_mut(&mut self) -> IterMut<T> {
        self.0.iter_mut()
    }
}

impl<'a, T> IntoIterator for &'a Row<T> {
    type Item = &'a T;
    type IntoIter = slice::Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> slice::Iter<'a, T> {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Row<T> {
    type Item = &'a mut T;
    type IntoIter = slice::IterMut<'a, T>;

    #[inline]
    fn into_iter(mut self) -> slice::IterMut<'a, T> {
        self.iter_mut()
    }
}

impl<T> Deref for Row<T> {
    type Target = Vec<T>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Row<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Index<index::Column> for Row<T> {
    type Output = T;

    #[inline]
    fn index(&self, index: index::Column) -> &T {
        &self.0[index.0]
    }
}

impl<T> IndexMut<index::Column> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: index::Column) -> &mut T {
        &mut self.0[index.0]
    }
}

macro_rules! row_index_range {
    ($range:ty) => {
        impl<T> Index<$range> for Row<T> {
            type Output = [T];

            #[inline]
            fn index(&self, index: $range) -> &[T] {
                &self.0[index]
            }
        }

        impl<T> IndexMut<$range> for Row<T> {
            #[inline]
            fn index_mut(&mut self, index: $range) -> &mut [T] {
                &mut self.0[index]
            }
        }
    }
}

row_index_range!(Range<usize>);
row_index_range!(RangeTo<usize>);
row_index_range!(RangeFrom<usize>);
row_index_range!(RangeFull);

// -----------------------------------------------------------------------------
// Row ranges for Grid
// -----------------------------------------------------------------------------

impl<T> Index<Range<index::Line>> for Grid<T> {
    type Output = [Row<T>];

    #[inline]
    fn index(&self, index: Range<index::Line>) -> &[Row<T>] {
        &self.raw[(index.start.0)..(index.end.0)]
    }
}

impl<T> IndexMut<Range<index::Line>> for Grid<T> {
    #[inline]
    fn index_mut(&mut self, index: Range<index::Line>) -> &mut [Row<T>] {
        &mut self.raw[(index.start.0)..(index.end.0)]
    }
}

impl<T> Index<RangeTo<index::Line>> for Grid<T> {
    type Output = [Row<T>];

    #[inline]
    fn index(&self, index: RangeTo<index::Line>) -> &[Row<T>] {
        &self.raw[..(index.end.0)]
    }
}

impl<T> IndexMut<RangeTo<index::Line>> for Grid<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeTo<index::Line>) -> &mut [Row<T>] {
        &mut self.raw[..(index.end.0)]
    }
}

impl<T> Index<RangeFrom<index::Line>> for Grid<T> {
    type Output = [Row<T>];

    #[inline]
    fn index(&self, index: RangeFrom<index::Line>) -> &[Row<T>] {
        &self.raw[(index.start.0)..]
    }
}

impl<T> IndexMut<RangeFrom<index::Line>> for Grid<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeFrom<index::Line>) -> &mut [Row<T>] {
        &mut self.raw[(index.start.0)..]
    }
}

// -----------------------------------------------------------------------------
// Column ranges for Row
// -----------------------------------------------------------------------------

impl<T> Index<Range<index::Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: Range<index::Column>) -> &[T] {
        &self.0[(index.start.0)..(index.end.0)]
    }
}

impl<T> IndexMut<Range<index::Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: Range<index::Column>) -> &mut [T] {
        &mut self.0[(index.start.0)..(index.end.0)]
    }
}

impl<T> Index<RangeTo<index::Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: RangeTo<index::Column>) -> &[T] {
        &self.0[..(index.end.0)]
    }
}

impl<T> IndexMut<RangeTo<index::Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeTo<index::Column>) -> &mut [T] {
        &mut self.0[..(index.end.0)]
    }
}

impl<T> Index<RangeFrom<index::Column>> for Row<T> {
    type Output = [T];

    #[inline]
    fn index(&self, index: RangeFrom<index::Column>) -> &[T] {
        &self.0[(index.start.0)..]
    }
}

impl<T> IndexMut<RangeFrom<index::Column>> for Row<T> {
    #[inline]
    fn index_mut(&mut self, index: RangeFrom<index::Column>) -> &mut [T] {
        &mut self.0[(index.start.0)..]
    }
}

pub trait ClearRegion<R, T> {
    fn clear_region<F: Fn(&mut T)>(&mut self, region: R, func: F);
}

macro_rules! clear_region_impl {
    ($range:ty) => {
        impl<T> ClearRegion<$range, T> for Grid<T> {
            fn clear_region<F: Fn(&mut T)>(&mut self, region: $range, func: F) {
                for row in self[region].iter_mut() {
                    for cell in row {
                        func(cell);
                    }
                }
            }
        }
    }
}

clear_region_impl!(Range<index::Line>);
clear_region_impl!(RangeTo<index::Line>);
clear_region_impl!(RangeFrom<index::Line>);

#[cfg(test)]
mod tests {
    use super::Grid;
    use index::{Line, Column};
    #[test]
    fn grid_swap_lines_ok() {
        let mut grid = Grid::new(Line(10), Column(1), &0);
        println!("");

        // swap test ends
        grid[Line(0)][Column(0)] = 1;
        grid[Line(9)][Column(0)] = 2;

        assert_eq!(grid[Line(0)][Column(0)], 1);
        assert_eq!(grid[Line(9)][Column(0)], 2);

        grid.swap_lines(Line(0), Line(9));

        assert_eq!(grid[Line(0)][Column(0)], 2);
        assert_eq!(grid[Line(9)][Column(0)], 1);

        // swap test mid
        grid[Line(4)][Column(0)] = 1;
        grid[Line(5)][Column(0)] = 2;

        println!("grid: {:?}", grid);

        assert_eq!(grid[Line(4)][Column(0)], 1);
        assert_eq!(grid[Line(5)][Column(0)], 2);

        grid.swap_lines(Line(4), Line(5));

        println!("grid: {:?}", grid);

        assert_eq!(grid[Line(4)][Column(0)], 2);
        assert_eq!(grid[Line(5)][Column(0)], 1);
    }

    #[test]
    #[should_panic]
    fn grid_swap_lines_oob1() {
        let mut grid = Grid::new(Line(10), Column(1), &0);
        grid.swap_lines(Line(0), Line(10));
    }

    #[test]
    #[should_panic]
    fn grid_swap_lines_oob2() {
        let mut grid = Grid::new(Line(10), Column(1), &0);
        grid.swap_lines(Line(10), Line(0));
    }

    #[test]
    #[should_panic]
    fn grid_swap_lines_oob3() {
        let mut grid = Grid::new(Line(10), Column(1), &0);
        grid.swap_lines(Line(10), Line(10));
    }

    // Scroll up moves lines upwards
    #[test]
    fn scroll_up() {
        println!("");

        let mut grid = Grid::new(Line(10), Column(1), &0);
        for i in 0..10 {
            grid[Line(i)][Column(0)] = i;
        }

        println!("grid: {:?}", grid);

        grid.scroll_up(Line(0)..Line(8), Line(2));

        println!("grid: {:?}", grid);

        let mut other = Grid::new(Line(10), Column(1), &9);

        other[Line(0)][Column(0)] = 2;
        other[Line(1)][Column(0)] = 3;
        other[Line(2)][Column(0)] = 4;
        other[Line(3)][Column(0)] = 5;
        other[Line(4)][Column(0)] = 6;
        other[Line(5)][Column(0)] = 7;
        other[Line(6)][Column(0)] = 8;
        other[Line(7)][Column(0)] = 9;
        other[Line(8)][Column(0)] = 0;
        other[Line(9)][Column(0)] = 1;

        for i in 0..10 {
            assert_eq!(grid[Line(i)][Column(0)], other[Line(i)][Column(0)]);
        }
    }

    // Scroll down moves lines downwards
    #[test]
    fn scroll_down() {
        println!("");

        let mut grid = Grid::new(Line(10), Column(1), &0);
        for i in 0..10 {
            grid[Line(i)][Column(0)] = i;
        }

        println!("grid: {:?}", grid);

        grid.scroll_down(Line(2)..Line(10), Line(2));

        println!("grid: {:?}", grid);

        let mut other = Grid::new(Line(10), Column(1), &9);

        other[Line(0)][Column(0)] = 8;
        other[Line(1)][Column(0)] = 9;
        other[Line(2)][Column(0)] = 0;
        other[Line(3)][Column(0)] = 1;
        other[Line(4)][Column(0)] = 2;
        other[Line(5)][Column(0)] = 3;
        other[Line(6)][Column(0)] = 4;
        other[Line(7)][Column(0)] = 5;
        other[Line(8)][Column(0)] = 6;
        other[Line(9)][Column(0)] = 7;

        for i in 0..10 {
            assert_eq!(grid[Line(i)][Column(0)], other[Line(i)][Column(0)]);
        }
    }
}
