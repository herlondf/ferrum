//! Strings of the toolbar, the tab bar, the status bar and the toasts.

pub static T: &[(&str, &str)] = &[
    ("No stashes", "Nenhum stash"),
    (
        "Check out a branch (Ctrl+B)",
        "Fazer checkout de uma branch (Ctrl+B)",
    ),
    ("Upstream branch", "Branch upstream"),
    ("not published", "não publicada"),
    (
        "The branch has no upstream yet: Push publishes it",
        "A branch ainda não tem upstream: o Push a publica",
    ),
    ("Up to date with the upstream", "Em dia com o upstream"),
    (
        "This repository uses Git LFS",
        "Este repositório usa Git LFS",
    ),
    ("never fetched", "nunca buscado"),
    ("Last fetch: {}", "Último fetch: {}"),
    ("Dismiss", "Dispensar"),
    ("Fetched {}", "Fetch de {} concluído"),
    ("Fetched {} remotes", "Fetch de {} remotos concluído"),
    ("Pulled {} from {}", "{} recebidos de {}"),
    ("Pulled from {}", "Pull de {} concluído"),
    ("Pulled from {}/{}", "Pull de {}/{} concluído"),
    ("Published {} to {}", "{} publicada em {}"),
    ("Pushed {} to {}", "{} enviados para {}"),
    ("Pushed branch {} to {}", "Branch {} enviada para {}"),
    ("Pushed {} branches to {}", "{} branches enviadas para {}"),
    ("Committed “{}”", "Commit “{}” criado"),
    ("Amended “{}”", "Commit “{}” corrigido"),
    (
        "Stashed the local changes",
        "Alterações locais guardadas no stash",
    ),
    ("Popped {}", "Stash “{}” aplicado e removido"),
    ("Switched to {}", "Agora em {}"),
    ("Created branch {}", "Branch {} criada"),
];
