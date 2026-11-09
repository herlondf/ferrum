# Ferrum benchmarks

Reproduce with `scripts/benchmark.sh` (see the comment at its top). It clones
the repositories with full history into `target/bench-repos`, runs
`ferrum --benchmark`, measures memory with GNU time and, when a display is
available, times the GUI from launch until the loaded history is on screen
(`FERRUM_SCREENSHOT`). Results go to `target/bench/RESULTS-<name>.md`.

```sh
scripts/benchmark.sh                # git/git, 5 runs
scripts/benchmark.sh --runs 3 linux # the Linux kernel (several GB to clone)
scripts/benchmark.sh ~/code/myrepo  # any local repository
```

The site shows the numbers below. Only Ferrum was measured; we have no
numbers for other clients.

## git/git, 2026-11-09

- Ferrum: commit 36d43ba (release build)
- Machine: Linux x86_64, AMD Ryzen 9 9950X 16-Core, 60 GB RAM, SSD, warm disk cache
- Repository: https://github.com/git/git at fb255012ac
- Size: 86,059 commits, 1,022 refs, 4,864 tracked files

```
step                       best    average  loaded
open                     0.2 ms     0.2 ms
head                     0.0 ms     0.0 ms
branches                 0.5 ms     0.5 ms  9
tags                     9.4 ms     9.7 ms  1011
remotes                  0.1 ms     0.1 ms  1
ref labels               0.3 ms     0.4 ms  1010
commits                461.2 ms   533.2 ms  10000
graph layout            12.7 ms    17.9 ms  196 lanes
stashes                  0.8 ms     2.2 ms  0
submodules               3.0 ms     3.7 ms  1
worktrees                0.5 ms     0.9 ms  1
lfs check                0.0 ms     0.0 ms  false
identity                 0.0 ms     0.0 ms
status                   6.5 ms     7.6 ms  0 staged, 0 unstaged
all commits            199.2 ms   238.7 ms  86059
all graph layout       199.3 ms   229.8 ms  282 lanes
path search           5000.7 ms  5237.9 ms  641
full reload (app)      492.5 ms   517.1 ms
text search (page)       2.4 ms     2.6 ms  1588
```

- Peak memory of a full `--benchmark` load (1 run, includes the whole history
  and the path search): 1084 MB
- GUI start to loaded history on screen (median of 5): 0.74 s, 350 MB max RSS

### Reading the numbers

- `commits` is the first page the app shows (10,000 commits). It is the first
  step that reads commit objects, so it pays for opening the pack files;
  `all commits` runs later on warm caches, which is why it reads all 86,059
  commits faster than the first page.
- `path search` is `git log --name-only` over the whole history of HEAD (the
  "file path" search mode), so it is bound by git itself.
- `text search (page)` filters the loaded page by message, author and hash,
  which is what happens while typing in the history search box.
- Not measured yet: the Linux kernel (supported: `scripts/benchmark.sh linux`)
  and scrolling frame times.
