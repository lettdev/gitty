# winget manifest (terminal app)

Templates for `winget install lettdev.Gitty`, which installs `gitty.exe` and
puts a `gitty` command on PATH (and installs Git for Windows if missing).

Needs, before the first submission:
- the GitHub release published (winget downloads from the public URL);

Submit: fill in the placeholders, copy the three files to
`manifests/l/lettdev/Gitty/<VERSION>/` in a fork of
https://github.com/microsoft/winget-pkgs, check with
`winget validate --manifest <folder>` and open a pull request there.
Later versions can be submitted automatically from the Release workflow.
