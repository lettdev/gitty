//! Lane assignment for a one-line-per-commit history graph.
//!
//! Each lane takes two character cells: the lane itself and a connector.

use crate::log::Commit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Lane index, used for coloring.
    pub lane: u16,
}

const BLANK: Cell = Cell { ch: ' ', lane: 0 };

#[derive(Clone, Debug, Default)]
pub struct GraphBuilder {
    /// The commit each lane is waiting for.
    lanes: Vec<Option<String>>,
}

fn cross(ch: char) -> char {
    match ch {
        ' ' | '─' => '─',
        '│' | '┤' | '├' => '┼',
        '╯' | '╰' => '┴',
        '╮' | '╭' => '┬',
        c => c,
    }
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute the graph cells for the next commit (commits must arrive in
    /// topological order, children first).
    pub fn push(&mut self, c: &Commit) -> Vec<Cell> {
        let prev = self.lanes.clone();
        let converging: Vec<usize> =
            prev.iter().enumerate().filter(|(_, l)| l.as_deref() == Some(c.hash.as_str())).map(|(i, _)| i).collect();
        let col = match converging.first() {
            Some(&i) => i,
            None => match self.lanes.iter().position(Option::is_none) {
                Some(i) => i,
                None => {
                    self.lanes.push(None);
                    self.lanes.len() - 1
                }
            },
        };
        let merged_in: Vec<usize> = converging.iter().copied().filter(|&i| i != col).collect();
        for &i in &merged_in {
            self.lanes[i] = None;
        }
        self.lanes[col] = c.parents.first().cloned();

        let mut opened = Vec::new();
        let mut joined = Vec::new();
        for p in c.parents.iter().skip(1) {
            if let Some(j) = self.lanes.iter().position(|l| l.as_deref() == Some(p.as_str())) {
                joined.push(j);
                continue;
            }
            let free = (col + 1..self.lanes.len())
                .find(|&k| self.lanes[k].is_none())
                .or_else(|| self.lanes.iter().position(Option::is_none));
            let j = match free {
                Some(k) => k,
                None => {
                    self.lanes.push(None);
                    self.lanes.len() - 1
                }
            };
            self.lanes[j] = Some(p.clone());
            opened.push(j);
        }

        let width = prev.len().max(self.lanes.len()).max(col + 1);
        let mut cells = vec![BLANK; width * 2];
        for (i, lane) in prev.iter().enumerate() {
            if i != col && lane.is_some() && !merged_in.contains(&i) {
                cells[2 * i] = Cell { ch: '│', lane: i as u16 };
            }
        }
        cells[2 * col] = Cell { ch: '●', lane: col as u16 };

        let mut connect = |target: usize, right: char, left: char| {
            let (lo, hi) = if target > col { (2 * col + 1, 2 * target) } else { (2 * target + 1, 2 * col) };
            for cell in &mut cells[lo..hi] {
                if cell.ch != '●' {
                    *cell = Cell { ch: cross(cell.ch), lane: target as u16 };
                }
            }
            cells[2 * target] = Cell { ch: if target > col { right } else { left }, lane: target as u16 };
        };
        let mut targets: Vec<(usize, char, char)> = Vec::new();
        targets.extend(merged_in.iter().map(|&i| (i, '╯', '╰')));
        targets.extend(opened.iter().map(|&j| (j, '╮', '╭')));
        targets.extend(joined.iter().map(|&j| (j, '┤', '├')));
        // nearest first, so a farther line crossing a nearer end turns it into a tee
        targets.sort_by_key(|&(t, _, _)| t.abs_diff(col));
        for (t, r, l) in targets {
            connect(t, r, l);
        }

        while self.lanes.last().is_some_and(Option::is_none) {
            self.lanes.pop();
        }
        while cells.len() > 2 && cells[cells.len() - 1].ch == ' ' && cells[cells.len() - 2].ch == ' ' {
            cells.truncate(cells.len() - 2);
        }
        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(hash: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            refs: vec![],
            subject: String::new(),
        }
    }

    fn row(cells: &[Cell]) -> String {
        cells.iter().map(|c| c.ch).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn merge_and_branch() {
        // m merges b into a; a and b both come from root
        let mut g = GraphBuilder::new();
        let rows: Vec<String> = [c("m", &["a", "b"]), c("b", &["r"]), c("a", &["r"]), c("r", &[])]
            .iter()
            .map(|x| row(&g.push(x)))
            .collect();
        assert_eq!(rows, vec!["●─╮", "│ ●", "● │", "●─╯"]);
    }

    #[test]
    fn linear() {
        let mut g = GraphBuilder::new();
        let rows: Vec<String> = [c("b", &["a"]), c("a", &[])].iter().map(|x| row(&g.push(x))).collect();
        assert_eq!(rows, vec!["●", "●"]);
    }
}
