//! Strings of the sidebar.

pub static T: &[(&str, &str)] = &[
    ("🔍 Filter", "🔍 Filtrar"),
    ("Sort branches and tags", "Ordenar branches e tags"),
    ("Sort branches", "Ordenar branches"),
    ("Sort tags", "Ordenar tags"),
    ("Tags", "Tags"),
    ("Alphabetically", "Alfabeticamente"),
    ("Alphabetically, descending", "Alfabeticamente, decrescente"),
    ("Most recent commit", "Commit mais recente"),
    ("Folders first", "Pastas primeiro"),
    ("No matches.", "Nenhum resultado."),
    ("No stashes.", "Nenhum stash."),
    ("New tag…", "Nova tag…"),
    ("📌 Pinned", "📌 Fixados"),
    ("📌 Pin to the top", "📌 Fixar no topo"),
    ("Unpin", "Desafixar"),
    ("📌 Show {}", "📌 Mostrar {}"),
    // ----- Branches -----
    ("Local branch: {}", "Branch local: {}"),
    ("Remote branch: {}", "Branch remota: {}"),
    ("tracked by {}", "acompanhada por {}"),
    (
        "Local only: not published to a remote",
        "Somente local: não publicada em um remoto",
    ),
    (
        "⚠ Its upstream {} no longer exists",
        "⚠ O upstream {} não existe mais",
    ),
    (
        "Stop tracking the upstream",
        "Parar de acompanhar o upstream",
    ),
    ("Stop tracking for {}", "Parar de acompanhar em {}"),
    (
        "Double-click to check out",
        "Clique duas vezes para fazer checkout",
    ),
    (
        "Double-click to check out (detached)",
        "Clique duas vezes para fazer checkout (destacado)",
    ),
    (
        "Double-click to open in a new tab",
        "Clique duas vezes para abrir em uma nova aba",
    ),
    (
        "Double-click to initialize",
        "Clique duas vezes para inicializar",
    ),
    ("Tag: {}", "Tag: {}"),
    ("Move to folder", "Mover para a pasta"),
    ("(top level)", "(nível superior)"),
    ("New branch in {}…", "Nova branch em {}…"),
    (
        "Select stale local branches ({})",
        "Selecionar branches locais obsoletas ({})",
    ),
    (
        "Branches whose upstream branch was deleted",
        "Branches cuja branch upstream foi excluída",
    ),
    ("{} selected", "{} selecionadas"),
    ("Delete branches", "Excluir branches"),
    ("Delete {} branches", "Excluir {} branches"),
    (
        "Delete {} local branches? Commits that are only on them will be lost.\n\n{}",
        "Excluir {} branches locais? Commits que estão somente nelas serão perdidos.\n\n{}",
    ),
    // ----- Drag and drop -----
    (
        "Check out {} and merge {} into it?",
        "Fazer checkout de {} e fazer merge de {} nela?",
    ),
    ("Rebase {} onto {}", "Fazer rebase de {} sobre {}"),
];
