use crate::git::Result;
use crate::repo::Repo;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Branch {
    /// Short name, e.g. `main` or `origin/main`.
    pub name: String,
    /// Full ref, e.g. `refs/heads/main`.
    pub full: String,
    pub remote: bool,
    pub head: bool,
    pub oid: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// The upstream branch was deleted on the remote.
    pub gone: bool,
    pub time: i64,
    pub subject: String,
}

impl Branch {
    /// For a remote branch `origin/feat/x`, the local name `feat/x`.
    pub fn local_name(&self) -> &str {
        if self.remote { self.name.split_once('/').map(|(_, b)| b).unwrap_or(&self.name) } else { &self.name }
    }
}

fn parse_track(track: &str) -> (u32, u32, bool) {
    let mut ahead = 0;
    let mut behind = 0;
    let gone = track.contains("gone");
    for part in track.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind, gone)
}

impl Repo {
    pub fn branches(&self) -> Result<Vec<Branch>> {
        let format = "--format=%(refname)%1f%(refname:short)%1f%(objectname:short)%1f%(upstream:short)%1f%(upstream:track,nobracket)%1f%(HEAD)%1f%(committerdate:unix)%1f%(contents:subject)";
        let out = self.git.run_text(&["for-each-ref", "--sort=-committerdate", format, "refs/heads", "refs/remotes"])?;
        let mut branches = Vec::new();
        for line in out.lines() {
            let f: Vec<&str> = line.splitn(8, '\x1f').collect();
            if f.len() < 8 || f[0].ends_with("/HEAD") {
                continue;
            }
            let (ahead, behind, gone) = parse_track(f[4]);
            branches.push(Branch {
                full: f[0].to_string(),
                name: f[1].to_string(),
                remote: f[0].starts_with("refs/remotes/"),
                oid: f[2].to_string(),
                upstream: Some(f[3].to_string()).filter(|u| !u.is_empty()),
                ahead,
                behind,
                gone,
                head: f[5] == "*",
                time: f[6].parse().unwrap_or(0),
                subject: f[7].to_string(),
            });
        }
        Ok(branches)
    }

    /// Tags, newest first: (name, short object id, unix time).
    pub fn tags(&self) -> Result<Vec<(String, String, i64)>> {
        let out = self.git.run_text(&[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)%1f%(objectname:short)%1f%(creatordate:unix)",
            "refs/tags",
        ])?;
        Ok(out
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.splitn(3, '\x1f').collect();
                (f.len() == 3).then(|| (f[0].to_string(), f[1].to_string(), f[2].parse().unwrap_or(0)))
            })
            .collect())
    }

    pub fn remotes(&self) -> Result<Vec<String>> {
        Ok(self.git.run_text(&["remote"])?.lines().map(str::to_string).filter(|r| !r.is_empty()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track() {
        assert_eq!(parse_track("ahead 2, behind 3"), (2, 3, false));
        assert_eq!(parse_track("gone"), (0, 0, true));
        assert_eq!(parse_track(""), (0, 0, false));
    }
}
