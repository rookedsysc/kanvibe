# KanVibe SSH hook lock recovery QA

Source PR: https://github.com/rookedsysc/kanvibe/pull/442
Verified source commit: ee4aff08f7ae749bb12780f4ddc458f0128ac3bc
Gate base: 9f48b2fb38ea41d73c6ed1a59c510bddb3d85542
Verified diff fingerprint: 0896381805e178fefc8f01caa26d06b30ad7fedc

Local pnpm check / pnpm test / pnpm build: exit0,201test files2792tests. Changed regression suites188tests. Real dev-vm/Electron default concurrency: terminal,tmux,herdr initial+reconnect6 roundtrips and Herdr→tmux switch1 roundtrip. UI loading images included. Own resources cleaned.

Bounded limitation: background hook status queries logged4 SSH reset/cooldown errors; server MaxStartups drops remained. SSH concurrency tuning is excluded from this PR. App deployment not performed.

Separate child-scoped concurrency1 lock smoke: exact whitespace/0600,8MiBcurrent-file frame,nestedSSH callbackonce,6concurrentupdates,ownedprocess SIGKILL→remote lockremoved/targetunpublished; all4checks pass.
