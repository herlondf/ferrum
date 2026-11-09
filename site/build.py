#!/usr/bin/env python3
"""Builds the website's pages from template.html, in English and pt-BR.

    python3 site/build.py           writes site/index.html and site/pt-BR/index.html
    python3 site/build.py --check   fails when they are out of date (CI)

Only the standard library. Every text of the page is in TEXT below, once per
language; the table, cards and lists are built from the data next to it.
Benchmark numbers come from `scripts/benchmark.sh` (see site/benchmarks.md);
update PERF when you rerun it.
"""

import html
import re
import sys
from pathlib import Path

SITE = Path(__file__).resolve().parent
RELEASES = "https://github.com/herlondf/ferrum/releases/latest/download/"
GET_SH = "curl -fsSL https://raw.githubusercontent.com/herlondf/ferrum/main/get.sh | sh"
GET_PS1 = "irm https://raw.githubusercontent.com/herlondf/ferrum/main/get.ps1 | iex"

# 24px stroke icons (Phosphor-like), inlined so the page needs no icon font.
ICONS = {
    "globe": '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3z"/>',
    "moon": '<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>',
    "sun": '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
    "github": '<path d="M9 19c-4.3 1.4-4.3-2.5-6-3m12 5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1.1-.3-3.5 1.3a12.3 12.3 0 0 0-6.2 0C6.5 2.8 5.4 3.1 5.4 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21"/>',
    "download": '<path d="M12 3v12M7 10l5 5 5-5M5 21h14"/>',
    "copy": '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/>',
    "graph": '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="9" r="2"/><path d="M6 7v10M18 11c0 4-6 3-11 6.5"/>',
    "lines": '<path d="M4 6h10M4 12h16M4 18h7"/><path d="M17 15l2 2 3-4"/>',
    "rebase": '<path d="M7 4v16M7 8h6a4 4 0 0 1 4 4v8"/><path d="M14 17l3 3 3-3"/>',
    "conflict": '<path d="M6 3v6a6 6 0 0 0 6 6 6 6 0 0 0 6-6V3"/><path d="M12 15v6"/>',
    "pr": '<circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M6 8v8M18 16V9a3 3 0 0 0-3-3h-4M13 4l-2 2 2 2"/>',
    "terminal": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/>',
    "palette": '<path d="M9 6h6M9 6a3 3 0 1 0-3 3h0v6a3 3 0 1 0 3 3h6a3 3 0 1 0 3-3V9a3 3 0 1 0-3-3"/>',
    "undo": '<path d="M9 14L4 9l5-5"/><path d="M4 9h11a5 5 0 0 1 0 10h-3"/>',
    "layers": '<path d="M12 3l9 5-9 5-9-5 9-5z"/><path d="M3 13l9 5 9-5"/>',
    "tree": '<path d="M12 3v18M12 8l-5 4M12 12l5 4"/>',
    "merge": '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="12" r="2"/><path d="M6 7v10M6 7c0 4 5 5 10 5"/>',
    "stack": '<rect x="4" y="4" width="16" height="5" rx="1.5"/><rect x="4" y="10" width="16" height="5" rx="1.5"/><rect x="4" y="16" width="16" height="4" rx="1.5"/>',
    "clock": '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    "feather": '<path d="M20 4c-6 0-12 4-14 12l-2 4M8 16h6c3 0 5-3 6-6"/>',
    "linux": '<path d="M12 3c-2.5 0-3.5 2-3.5 4.5 0 2-1 3-2 4.5S4 15 5 17s4 3 7 3 6-1 7-3-.5-3.5-1.5-5-2-2.5-2-4.5C15.5 5 14.5 3 12 3z"/><circle cx="10.5" cy="8" r=".6"/><circle cx="13.5" cy="8" r=".6"/>',
    "windows": '<path d="M3 5.5l7.5-1v7H3zM12 4.3L21 3v8.5h-9zM3 12.5h7.5v7L3 18.5zM12 12.5h9V21l-9-1.3z"/>',
    "apple": '<path d="M16.5 3c-1.2.1-2.6.9-3.3 1.9-.7.8-1.2 2-1 3.2 1.3.1 2.6-.7 3.3-1.6.7-.9 1.2-2.1 1-3.5zM20 16.8c-.6 1.3-.9 1.9-1.7 3-1.1 1.6-2.6 3.5-4.5 3.5-1.7 0-2.1-1.1-4.4-1.1s-2.8 1.1-4.5 1.1c-1.9 0-3.3-1.8-4.4-3.4C-1.6 15.4 1.3 9 5.2 9c1.7 0 2.8 1.1 4.2 1.1 1.4 0 2.2-1.1 4.3-1.1 1.5 0 3.1.8 4.3 2.3-3.8 2.1-3.2 7.5 2 5.5z" transform="scale(.92) translate(1.5 -1)"/>',
}


def icon(name: str) -> str:
    return (
        '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" '
        f'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{ICONS[name]}</svg>'
    )


# Measured with scripts/benchmark.sh on git/git; see site/benchmarks.md.
PERF = {
    "commits": "86,059",
    "gui_start": "0.74",
    "gui_rss": "350",
    "first_page": "0.49",
    "all": "0.40",
    "search": "2.4",
    "raw": """git/git at fb255012ac: 86,059 commits, 1,022 refs, 4,864 files
Linux x86_64, AMD Ryzen 9 9950X, 60 GB RAM, SSD, warm disk cache
`ferrum --benchmark` (best of 5)

step                       best    average  loaded
open                     0.2 ms     0.2 ms
branches                 0.5 ms     0.5 ms  9
tags                     9.4 ms     9.7 ms  1011
commits                461.2 ms   533.2 ms  10000
graph layout            12.7 ms    17.9 ms  196 lanes
status                   6.5 ms     7.6 ms  0 staged, 0 unstaged
all commits            199.2 ms   238.7 ms  86059
all graph layout       199.3 ms   229.8 ms  282 lanes
path search           5000.7 ms  5237.9 ms  641
full reload (app)      492.5 ms   517.1 ms
text search (page)       2.4 ms     2.6 ms  1588

GUI start to loaded history on screen (median of 5): 0.74 s, 350 MB max RSS""",
}

# Comparison table. Cells: (class, text key or literal). Classes: y = yes,
# n = no, q = not verified / not stated, s = coming soon, "" = plain text.
# Competitor facts were checked on the vendors' websites (Nov 2026) where the
# site answered; "q" marks what we could not confirm. Keep it that way:
# never fill a cell without a source.
COMPARE = [
    ("row_price", [("", "c_free"), ("", "c_fork_price"), ("", "c_gk_price"), ("", "c_free")]),
    ("row_linux", [("y", "c_yes"), ("n", "c_no"), ("y", "c_yes_dag"), ("n", "c_no")]),
    ("row_windows", [("y", "c_yes"), ("y", "c_yes"), ("y", "c_yes_dag"), ("y", "c_yes")]),
    ("row_macos", [("y", "c_yes"), ("y", "c_yes"), ("y", "c_yes_dag"), ("y", "c_yes")]),
    ("row_oss", [("y", "c_oss"), ("n", "c_no"), ("n", "c_no"), ("n", "c_no")]),
    ("row_account", [("y", "c_no_account"), ("q", "c_not_stated"), ("n", "c_account_dag"), ("q", "c_not_stated")]),
    ("row_rebase", [("y", "c_yes"), ("y", "c_yes"), ("q", "c_unverified"), ("y", "c_yes")]),
    ("row_lines", [("y", "c_yes"), ("y", "c_yes"), ("q", "c_unverified"), ("y", "c_yes")]),
    ("row_conflicts", [("y", "c_yes"), ("y", "c_yes"), ("q", "c_unverified"), ("q", "c_not_stated")]),
    ("row_prs", [("y", "c_prs"), ("q", "c_not_stated"), ("q", "c_unverified"), ("q", "c_not_stated")]),
    ("row_undo", [("y", "c_yes"), ("q", "c_not_stated"), ("q", "c_unverified"), ("q", "c_not_stated")]),
    ("row_activity", [("y", "c_yes"), ("q", "c_not_stated"), ("q", "c_unverified"), ("q", "c_not_stated")]),
    ("row_structural", [("y", "c_yes"), ("q", "c_not_stated"), ("q", "c_unverified"), ("q", "c_not_stated")]),
    ("row_speed", [("", "c_speed"), ("q", "c_not_measured"), ("q", "c_not_measured"), ("q", "c_not_measured")]),
]

TOUR = [("changes", "lines"), ("diff", "layers"), ("palette", "palette"), ("start", "stack"), ("history", "graph")]
FEATURES = [
    ("graph", "f_graph"), ("lines", "f_lines"), ("rebase", "f_rebase"),
    ("conflict", "f_conflict"), ("pr", "f_pr"), ("terminal", "f_activity"),
    ("undo", "f_undo"), ("palette", "f_palette"), ("feather", "f_native"),
]
SOON = [("tree", "s_structural"), ("merge", "s_semantic"), ("stack", "s_stacks"), ("clock", "s_timeline")]

TEXT = {
    "en": {
        "lang": "en",
        "title": "Ferrum: a fast, free and open source Git client",
        "description": "Ferrum is a native Git client for Linux, Windows and macOS. History graph, line staging, interactive rebase, conflict resolution and pull requests, free and open source.",
        "other_lang": "pt-BR",
        "other_label": "Português",
        "other_title": "Ver em português",
        "theme_toggle": "Switch light / dark",
        "nav_tour": "Tour",
        "nav_features": "Features",
        "nav_compare": "Compare",
        "nav_perf": "Performance",
        "nav_download": "Download",
        "badge_new": "Free",
        "badge": "and open source · Linux, Windows, macOS",
        "hero_title": "Git, <em>fast</em>.<br>Without the subscription.",
        "hero_lead": "Ferrum is a native Git client written in Rust. A clear history graph, staging down to single lines, interactive rebase by drag and drop and a three-way conflict resolver, with no account and no telemetry.",
        "hero_download": "Download",
        "hero_source": "Source code",
        "hero_note": "MIT or Apache-2.0 · No sign-in · English and Portuguese",
        "alt_history": "Ferrum showing the history graph of a repository, with commit details and a diff",
        "tour_eyebrow": "Tour",
        "tour_title": "Everything you do every day, one click away",
        "tour_lead": "Real screenshots of Ferrum, in the theme you are using now. They change by themselves; click to pick one.",
        "tour_history": ("History graph", "Branches in colored lanes, labels for branches and tags, search by message, author, hash, content or path."),
        "tour_changes": ("Local changes", "Stage files, hunks or single lines, then write the message with a live length counter."),
        "tour_diff": ("Diffs", "Side by side or inline, syntax colors, word-level changes and a large view with Space."),
        "tour_palette": ("Command palette", "Ctrl+P finds any command, branch or file, with the shortcut next to it."),
        "tour_start": ("Repositories", "Open, clone or create from the start screen, with every repository as a card."),
        "feat_eyebrow": "Features",
        "feat_title": "Built for daily work on real repositories",
        "feat_lead": "Ferrum reads with libgit2 for speed and writes with your own git, so hooks, credential helpers, SSH and LFS behave exactly as in the terminal.",
        "f_graph": ("History that stays readable", "Colored lanes, collapsible merges, first-parent and topological order, reflog, and branch filters saved per repository."),
        "f_lines": ("Stage lines, not just files", "Pick hunks or single lines in the diff, discard what you don't want, amend, sign off and push after commit."),
        "f_rebase": ("Interactive rebase by drag and drop", "Reorder, squash, fix up, reword and drop commits in a planner, with a preview of conflicts before running."),
        "f_conflict": ("Three-way conflict resolver", "Choose per conflict, edit the result, resolve images, LFS files and submodules, and undo a resolution."),
        "f_pr": ("Pull requests and issues", "Accounts for GitHub, GitLab, Bitbucket, Azure DevOps and Gitea: list, check out and create pull requests, browse issues, see GitHub notifications."),
        "f_activity": ("Nothing hidden", "The Activity window shows every git command Ferrum ran, with its output. Long operations can be cancelled."),
        "f_undo": ("Undo", "Ctrl+Z reverts commits, checkouts, branch and tag changes, merges and rebases."),
        "f_palette": ("Keyboard first", "A command palette, Open Quickly for repositories, and shortcuts you can change for everything."),
        "f_native": ("Native and light", "A Rust binary drawn on the GPU, no Electron. Dark, light and high-contrast themes, Inter and JetBrains Mono."),
        "soon_title": "New",
        "soon_lead": "Built into Ferrum, with no plugins or subscriptions.",
        "s_structural": ("Structural diff", "Diffs that understand the code's syntax, so moved and reformatted code reads as what it is."),
        "s_semantic": ("Semantic merge", "Conflicts resolved with the structure of the code in mind, not just lines."),
        "s_stacks": ("Stacked branches", "Work on a chain of dependent branches and keep them rebased with one action."),
        "s_timeline": ("Undo anything", "A timeline of every operation with restore points, so any change can be taken back."),
        "cmp_eyebrow": "Compare",
        "cmp_title": "How Ferrum compares",
        "cmp_lead": "We only fill a cell when we could check it. Competitor facts come from their websites; anything we could not confirm is marked as such. Corrections are welcome.",
        "row_price": "Price",
        "row_linux": "Linux",
        "row_windows": "Windows",
        "row_macos": "macOS",
        "row_oss": "Open source",
        "row_account": "Works without an account",
        "row_rebase": "Interactive rebase",
        "row_lines": "Stage single lines",
        "row_conflicts": "Built-in conflict resolver",
        "row_prs": "Pull requests in the app",
        "row_undo": "Undo operations",
        "row_activity": "Log of the exact git commands",
        "row_structural": "Structural (syntax-aware) diff",
        "row_speed": "Start to loaded history, git/git (86k commits)",
        "c_free": "Free",
        "c_fork_price": "USD 59.99 one-time, free evaluation",
        "c_gk_price": "Free plan and paid plans †",
        "c_yes": "Yes",
        "c_yes_dag": "Yes †",
        "c_no": "No",
        "c_oss": "Yes (MIT / Apache-2.0)",
        "c_no_account": "Yes",
        "c_account_dag": "Sign-in required †",
        "c_not_stated": "Not stated",
        "c_unverified": "Not verified",
        "c_prs": "GitHub, GitLab, Bitbucket, Azure DevOps, Gitea",
        "c_soon": "Coming soon",
        "c_speed": "0.74 s",
        "c_not_measured": "Not measured",
        "notes": [
            "“Not stated”: the vendor's website (git-fork.com, sourcetreeapp.com, checked November 2026) does not say either way.",
            "† GitKraken's website refused our automated check; these cells are from its public material as last known to us and may be out of date. “Not verified” cells were not checked at all.",
            "We did not benchmark the other clients. Ferrum's number is from scripts/benchmark.sh, below. Found a mistake? <a href=\"https://github.com/herlondf/ferrum/issues\">Open an issue</a>.",
        ],
        "perf_eyebrow": "Performance",
        "perf_title": "Measured, not promised",
        "perf_lead": "Numbers from <code>scripts/benchmark.sh</code> on git's own repository (86,059 commits), on one machine. Run the script on your repositories to get yours.",
        "p_gui": ("s", "From launch to the loaded history graph on screen"),
        "p_rss": ("MB", "Memory of the whole app with that repository open"),
        "p_first": ("s", "Reload: refs, the first 10,000 commits, graph and status"),
        "p_search": ("ms", "Filter 10,000 commits by text as you type"),
        "perf_more": "All the steps",
        "perf_note": "AMD Ryzen 9 9950X, Linux, SSD, warm disk cache; best of 5 runs. Scrolling to the very end loads all 86,059 commits in 0.2 s plus 0.2 s of graph layout. Larger repositories such as the Linux kernel are supported by the script (<code>scripts/benchmark.sh linux</code>) but not measured here yet.",
        "dl_eyebrow": "Download",
        "dl_title": "Get Ferrum",
        "dl_lead": "Free for everyone, for any use. The installers update themselves from the app; the packages update with your system.",
        "dl_linux": "Linux",
        "dl_windows": "Windows",
        "dl_macos": "macOS",
        "dl_appimage": "AppImage",
        "dl_deb": "Debian, Ubuntu",
        "dl_rpm": "Fedora, openSUSE",
        "dl_tar": "Binary (.tar.gz)",
        "dl_setup": "Installer (.exe)",
        "dl_zip": "Portable (.zip)",
        "dl_dmg": "Disk image (.dmg)",
        "dl_mac_zip": "App (.zip)",
        "dl_sh": "Or in a terminal, no root:",
        "dl_ps": "Or in PowerShell, no admin:",
        "dl_mac_note": "One app for Apple silicon and Intel. It is not notarized yet: the first time, right-click it and choose Open.",
        "dl_soon": "Flathub, winget, Scoop and Homebrew: coming soon.",
        "dl_note": "If a download link does not work yet, the first release has not been published: see <a href=\"https://github.com/herlondf/ferrum/releases\">all releases</a> or build from source with <code>cargo build --release</code>. Every release lists SHA-256 checksums in <code>SHA256SUMS</code>.",
        "copy": "Copy",
        "footer": "Ferrum is free software, MIT or Apache-2.0.",
        "footer_releases": "Releases",
        "footer_issues": "Issues",
    },
    "pt-BR": {
        "lang": "pt-BR",
        "title": "Ferrum: um cliente Git rápido, gratuito e de código aberto",
        "description": "Ferrum é um cliente Git nativo para Linux, Windows e macOS. Grafo do histórico, stage de linhas, rebase interativo, resolução de conflitos e pull requests, gratuito e de código aberto.",
        "other_lang": "en",
        "other_label": "English",
        "other_title": "View in English",
        "theme_toggle": "Alternar claro / escuro",
        "nav_tour": "Tour",
        "nav_features": "Recursos",
        "nav_compare": "Comparação",
        "nav_perf": "Desempenho",
        "nav_download": "Download",
        "badge_new": "Gratuito",
        "badge": "e de código aberto · Linux, Windows, macOS",
        "hero_title": "Git, <em>rápido</em>.<br>Sem assinatura.",
        "hero_lead": "Ferrum é um cliente Git nativo escrito em Rust. Um grafo de histórico claro, stage até de linhas soltas, rebase interativo arrastando commits e um resolvedor de conflitos de três vias, sem conta e sem telemetria.",
        "hero_download": "Baixar",
        "hero_source": "Código-fonte",
        "hero_note": "MIT ou Apache-2.0 · Sem login · Português e inglês",
        "alt_history": "Ferrum mostrando o grafo do histórico de um repositório, com os detalhes do commit e um diff",
        "tour_eyebrow": "Tour",
        "tour_title": "Tudo o que você faz no dia a dia, a um clique",
        "tour_lead": "Capturas de tela reais do Ferrum, no tema que você está usando agora. Elas trocam sozinhas; clique para escolher.",
        "tour_history": ("Grafo do histórico", "Branches em faixas coloridas, rótulos de branches e tags, busca por mensagem, autor, hash, conteúdo ou caminho."),
        "tour_changes": ("Alterações locais", "Faça stage de arquivos, trechos ou linhas soltas e escreva a mensagem com um contador de tamanho."),
        "tour_diff": ("Diffs", "Lado a lado ou em linha, com cores de sintaxe, mudanças por palavra e uma visão ampliada com Espaço."),
        "tour_palette": ("Paleta de comandos", "Ctrl+P encontra qualquer comando, branch ou arquivo, com o atalho ao lado."),
        "tour_start": ("Repositórios", "Abra, clone ou crie pela tela inicial, com cada repositório num cartão."),
        "feat_eyebrow": "Recursos",
        "feat_title": "Feito para o trabalho diário em repositórios de verdade",
        "feat_lead": "O Ferrum lê com a libgit2, pela velocidade, e escreve com o seu próprio git, então hooks, credential helpers, SSH e LFS funcionam exatamente como no terminal.",
        "f_graph": ("Histórico que continua legível", "Faixas coloridas, merges recolhíveis, ordem first-parent e topológica, reflog e filtros de branches salvos por repositório."),
        "f_lines": ("Stage de linhas, não só de arquivos", "Escolha trechos ou linhas soltas no diff, descarte o que não quer, faça amend, sign-off e push depois do commit."),
        "f_rebase": ("Rebase interativo arrastando", "Reordene, junte (squash), corrija (fixup), reescreva e remova commits num planejador, com prévia de conflitos antes de rodar."),
        "f_conflict": ("Resolvedor de conflitos de três vias", "Escolha por conflito, edite o resultado, resolva imagens, arquivos LFS e submódulos, e desfaça uma resolução."),
        "f_pr": ("Pull requests e issues", "Contas do GitHub, GitLab, Bitbucket, Azure DevOps e Gitea: liste, faça checkout e crie pull requests, navegue pelas issues e veja as notificações do GitHub."),
        "f_activity": ("Nada escondido", "A janela Atividade mostra cada comando git que o Ferrum rodou, com a saída. Operações longas podem ser canceladas."),
        "f_undo": ("Desfazer", "Ctrl+Z desfaz commits, checkouts, mudanças de branches e tags, merges e rebases."),
        "f_palette": ("Teclado em primeiro lugar", "Uma paleta de comandos, Abrir rapidamente para repositórios e atalhos que você pode mudar para tudo."),
        "f_native": ("Nativo e leve", "Um binário Rust desenhado pela GPU, sem Electron. Temas escuro, claro e de alto contraste, Inter e JetBrains Mono."),
        "soon_title": "Novo",
        "soon_lead": "Já vem no Ferrum, sem plugins nem assinatura.",
        "s_structural": ("Diff estrutural", "Diffs que entendem a sintaxe do código, para que código movido ou reformatado apareça como realmente é."),
        "s_semantic": ("Merge semântico", "Conflitos resolvidos levando em conta a estrutura do código, não só as linhas."),
        "s_stacks": ("Branches empilhadas", "Trabalhe numa cadeia de branches dependentes e mantenha todas rebaseadas com uma ação."),
        "s_timeline": ("Desfazer qualquer coisa", "Uma linha do tempo de cada operação com pontos de restauração, para que qualquer mudança possa ser desfeita."),
        "cmp_eyebrow": "Comparação",
        "cmp_title": "Como o Ferrum se compara",
        "cmp_lead": "Só preenchemos uma célula quando conseguimos conferir. Os dados dos concorrentes vêm dos sites deles; o que não conseguimos confirmar está marcado. Correções são bem-vindas.",
        "row_price": "Preço",
        "row_linux": "Linux",
        "row_windows": "Windows",
        "row_macos": "macOS",
        "row_oss": "Código aberto",
        "row_account": "Funciona sem conta",
        "row_rebase": "Rebase interativo",
        "row_lines": "Stage de linhas soltas",
        "row_conflicts": "Resolvedor de conflitos embutido",
        "row_prs": "Pull requests no app",
        "row_undo": "Desfazer operações",
        "row_activity": "Registro dos comandos git exatos",
        "row_structural": "Diff estrutural (entende a sintaxe)",
        "row_speed": "Abrir até o histórico carregado, git/git (86 mil commits)",
        "c_free": "Gratuito",
        "c_fork_price": "USD 59,99 pagamento único, avaliação gratuita",
        "c_gk_price": "Plano gratuito e planos pagos †",
        "c_yes": "Sim",
        "c_yes_dag": "Sim †",
        "c_no": "Não",
        "c_oss": "Sim (MIT / Apache-2.0)",
        "c_no_account": "Sim",
        "c_account_dag": "Exige login †",
        "c_not_stated": "Não informado",
        "c_unverified": "Não verificado",
        "c_prs": "GitHub, GitLab, Bitbucket, Azure DevOps, Gitea",
        "c_soon": "Em breve",
        "c_speed": "0,74 s",
        "c_not_measured": "Não medido",
        "notes": [
            "“Não informado”: o site do fabricante (git-fork.com, sourcetreeapp.com, consultados em novembro de 2026) não diz nem sim nem não.",
            "† O site do GitKraken recusou nossa verificação automática; estas células vêm do material público dele como o conhecíamos e podem estar desatualizadas. Células “Não verificado” não foram conferidas.",
            "Não medimos o desempenho dos outros clientes. O número do Ferrum vem do scripts/benchmark.sh, abaixo. Achou um erro? <a href=\"https://github.com/herlondf/ferrum/issues\">Abra uma issue</a>.",
        ],
        "perf_eyebrow": "Desempenho",
        "perf_title": "Medido, não prometido",
        "perf_lead": "Números do <code>scripts/benchmark.sh</code> no próprio repositório do git (86.059 commits), numa máquina. Rode o script nos seus repositórios para ter os seus.",
        "p_gui": ("s", "Da abertura até o grafo do histórico carregado na tela"),
        "p_rss": ("MB", "Memória do app inteiro com esse repositório aberto"),
        "p_first": ("s", "Recarregar: refs, os primeiros 10.000 commits, grafo e status"),
        "p_search": ("ms", "Filtrar 10.000 commits por texto enquanto você digita"),
        "perf_more": "Todas as etapas",
        "perf_note": "AMD Ryzen 9 9950X, Linux, SSD, cache de disco quente; melhor de 5 execuções. Rolar até o fim carrega os 86.059 commits em 0,2 s, mais 0,2 s para desenhar o grafo. Repositórios maiores, como o kernel Linux, são suportados pelo script (<code>scripts/benchmark.sh linux</code>), mas ainda não foram medidos aqui.",
        "dl_eyebrow": "Download",
        "dl_title": "Baixe o Ferrum",
        "dl_lead": "Gratuito para todos, para qualquer uso. Os instaladores se atualizam pelo próprio app; os pacotes se atualizam com o sistema.",
        "dl_linux": "Linux",
        "dl_windows": "Windows",
        "dl_macos": "macOS",
        "dl_appimage": "AppImage",
        "dl_deb": "Debian, Ubuntu",
        "dl_rpm": "Fedora, openSUSE",
        "dl_tar": "Binário (.tar.gz)",
        "dl_setup": "Instalador (.exe)",
        "dl_zip": "Portátil (.zip)",
        "dl_dmg": "Imagem de disco (.dmg)",
        "dl_mac_zip": "App (.zip)",
        "dl_sh": "Ou no terminal, sem root:",
        "dl_ps": "Ou no PowerShell, sem administrador:",
        "dl_mac_note": "Um só app para Apple silicon e Intel. Ele ainda não é notarizado: na primeira vez, clique com o botão direito nele e escolha Abrir.",
        "dl_soon": "Flathub, winget, Scoop e Homebrew: em breve.",
        "dl_note": "Se um link de download ainda não funcionar, a primeira versão não foi publicada: veja <a href=\"https://github.com/herlondf/ferrum/releases\">todas as versões</a> ou compile o código com <code>cargo build --release</code>. Cada versão traz as somas SHA-256 em <code>SHA256SUMS</code>.",
        "copy": "Copiar",
        "footer": "Ferrum é software livre, MIT ou Apache-2.0.",
        "footer_releases": "Versões",
        "footer_issues": "Issues",
    },
}


def esc(s: str) -> str:
    return html.escape(s, quote=True)


def build(lang: str) -> str:
    t = TEXT[lang]
    root = "" if lang == "en" else "../"
    shots = "en" if lang == "en" else "pt"
    other = "pt-BR/" if lang == "en" else "../"

    tabs, figs = [], []
    for i, (key, ico) in enumerate(TOUR):
        title, text = t[f"tour_{key}"]
        sel = "true" if i == 0 else "false"
        tabs.append(
            f'        <button class="tour-tab" role="tab" aria-selected="{sel}" data-tab="{key}">'
            f"<h3>{title}</h3><p>{text}</p><span class=\"bar\"></span></button>"
        )
        active = " active" if i == 0 else ""
        lazy = "" if i == 0 else ' loading="lazy"'
        figs.append(
            f'        <figure class="{active.strip()}" data-fig="{key}">'
            f'<img class="shot only-dark" src="{root}assets/screenshots/{key}-{shots}-dark.webp" width="1440" height="900" alt="{esc(title)}"{lazy}>'
            f'<img class="shot only-light" src="{root}assets/screenshots/{key}-{shots}-light.webp" width="1440" height="900" alt="{esc(title)}" loading="lazy">'
            "</figure>"
        )

    def cards(items, extra=""):
        out = []
        for ico, key in items:
            title, text = t[key]
            out.append(
                f'        <div class="card"><div class="ico">{icon(ico)}</div>{extra}<h3>{title}</h3><p>{text}</p></div>'
            )
        return "\n".join(out)

    rows = []
    for label, cells in COMPARE:
        tds = []
        for i, (cls, key) in enumerate(cells):
            classes = " ".join(c for c in (cls, "us" if i == 0 else "") if c)
            attr = f' class="{classes}"' if classes else ""
            tds.append(f"<td{attr}>{t.get(key, key)}</td>")
        rows.append(f'          <tr><th scope="row">{t[label]}</th>{"".join(tds)}</tr>')
    notes = "\n".join(f"      <p>{n}</p>" for n in t["notes"])

    def number(v: str) -> str:
        return v.replace(".", ",") if lang == "pt-BR" else v

    stats = []
    for key, value in [("p_gui", PERF["gui_start"]), ("p_rss", PERF["gui_rss"]),
                       ("p_first", PERF["first_page"]), ("p_search", PERF["search"])]:
        unit, text = t[key]
        stats.append(
            f'      <div class="stat"><div class="num">{number(value)}<small>{unit}</small></div><p>{text}</p></div>'
        )

    copy_btn = f'<button type="button" data-copy title="{t["copy"]}" aria-label="{t["copy"]}">{icon("copy")}</button>'

    def dl(os_key, ico, main, items, extra):
        name, label = main
        lis = "".join(
            f'<li><a href="{RELEASES}{n}">{t[k]}</a><span>{n.split(".", 1)[1]}</span></li>' for k, n in items
        )
        return (
            f'      <div class="card dl" data-os="{os_key}"><h3>{icon(ico)}{t["dl_" + os_key]}</h3>'
            f'<a class="btn btn-primary" href="{RELEASES}{name}">{icon("download")}{t[label]}</a>'
            f"<ul>{lis}</ul>{extra}</div>"
        )

    downloads = "\n".join([
        dl("linux", "linux", ("ferrum-linux-x86_64.AppImage", "dl_appimage"),
           [("dl_deb", "ferrum-linux-x86_64.deb"), ("dl_rpm", "ferrum-linux-x86_64.rpm"),
            ("dl_tar", "ferrum-linux-x86_64.tar.gz")],
           f'<p class="muted">{t["dl_sh"]}</p><div class="cmd"><code>{esc(GET_SH)}</code>{copy_btn}</div>'),
        dl("windows", "windows", ("ferrum-windows-x86_64-setup.exe", "dl_setup"),
           [("dl_zip", "ferrum-windows-x86_64.zip")],
           f'<p class="muted">{t["dl_ps"]}</p><div class="cmd"><code>{esc(GET_PS1)}</code>{copy_btn}</div>'),
        dl("macos", "apple", ("ferrum-macos-universal.dmg", "dl_dmg"),
           [("dl_mac_zip", "ferrum-macos-universal.zip")],
           f'<p class="muted">{t["dl_mac_note"]}</p>'),
    ])

    values = dict(t)
    values.update({
        "root": root,
        "home": "./" if lang == "en" else "./",
        "shots": shots,
        "other_url": other,
        "tour_tabs": "\n".join(tabs),
        "tour_figures": "\n".join(figs),
        "features": cards(FEATURES),
        "soon": cards(SOON, f'<span class="pill">{t["soon_title"]}</span>'),
        "compare_rows": "\n".join(rows),
        "compare_notes": notes,
        "perf_stats": "\n".join(stats),
        "perf_raw": esc(PERF["raw"]),
        "downloads": downloads,
        "dl_note": t["dl_note"] + " " + t["dl_soon"],
    })
    page = (SITE / "template.html").read_text()

    def fill(m):
        key = m.group(1)
        if key.startswith("icon:"):
            return icon(key[5:])
        if key not in values:
            sys.exit(f"template uses unknown key {key}")
        return values[key]

    return re.sub(r"\{\{([a-z_:]+)\}\}", fill, page)


def main() -> None:
    check = "--check" in sys.argv
    stale = []
    for lang, out in (("en", SITE / "index.html"), ("pt-BR", SITE / "pt-BR/index.html")):
        page = build(lang)
        if check:
            if not out.exists() or out.read_text() != page:
                stale.append(str(out))
        else:
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text(page)
            print(out)
    if stale:
        sys.exit("out of date (run python3 site/build.py): " + ", ".join(stale))


if __name__ == "__main__":
    main()
