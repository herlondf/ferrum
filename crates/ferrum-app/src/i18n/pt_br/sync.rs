//! Strings of fetch, pull, push and remotes.

pub static T: &[(&str, &str)] = &[
    // ----- Toolbar -----
    ("Fetch", "Fetch"),
    ("Options… (or Alt+click)", "Opções… (ou Alt+clique)"),
    ("Pull — {} (Ctrl+Shift+D)", "Pull — {} (Ctrl+Shift+D)"),
    (
        "Push (Ctrl+Shift+U for options)",
        "Fazer push (Ctrl+Shift+U para opções)",
    ),
    ("{} commits to pull", "{} commits para receber"),
    ("{} commits to push", "{} commits para enviar"),
    ("Fetch with options…", "Fetch com opções…"),
    ("Push tags…", "Fazer push de tags…"),
    // ----- Operations -----
    ("Pull {}/{}", "Pull de {}/{}"),
    ("all tags", "todas as tags"),
    ("Fast-forward {} to {}", "Fast-forward de {} para {}"),
    ("Fast-forward {} branches", "Fast-forward de {} branches"),
    ("Edit remote {}", "Editar remoto {}"),
    ("Test connection to {}", "Testar conexão com {}"),
    // ----- Dialogs -----
    ("Running: {}", "Em execução: {}"),
    ("✖ Cancel it", "✖ Cancelar"),
    (
        "This one starts when it finishes.",
        "Esta começa quando ela terminar.",
    ),
    ("Remote", "Remoto"),
    (
        "Prune remote branches that were deleted",
        "Remover branches remotas que foram apagadas",
    ),
    ("Always fetch all tags", "Sempre buscar todas as tags"),
    (
        "Stash and reapply local changes",
        "Guardar no stash e reaplicar as alterações locais",
    ),
    ("Remote branch", "Branch remota"),
    ("{}/{} will be created.", "{}/{} será criada."),
    ("None", "Nenhuma"),
    ("Annotated, with the branch", "Anotadas, com a branch"),
    ("All tags", "Todas as tags"),
    ("Choose tags to push…", "Escolher tags para enviar…"),
    (
        "Refuse to push if submodule commits are not pushed",
        "Recusar o push se houver commits de submódulos sem push",
    ),
    ("{} ({} commits)", "{} ({} commits)"),
    (
        "⚠ Submodules with commits that are not pushed yet: {}. Push them first, or others cannot check out this commit.",
        "⚠ Submódulos com commits que ainda não foram enviados: {}. Faça push deles antes, ou outras pessoas não conseguirão fazer checkout deste commit.",
    ),
    ("Push {} branches", "Fazer push de {} branches"),
    ("All or nothing (atomic)", "Tudo ou nada (atômico)"),
    (
        "If one branch is rejected, none is updated (--atomic)",
        "Se uma branch for rejeitada, nenhuma é atualizada (--atomic)",
    ),
    (
        "Track the remote branches (set upstream)",
        "Acompanhar as branches remotas (definir upstream)",
    ),
    ("Push tags", "Fazer push de tags"),
    ("Filter…", "Filtrar…"),
    ("Select all", "Selecionar todas"),
    ("Select none", "Desmarcar todas"),
    ("Push {} tags", "Fazer push de {} tags"),
    ("Tracking branch of {}", "Branch acompanhada por {}"),
    (
        "There are no remote branches. Fetch first.",
        "Não há branches remotas. Faça fetch antes.",
    ),
    ("Track", "Acompanhar"),
    ("Pull from it now", "Fazer pull dela agora"),
    ("Fast-forward {} to it", "Fazer fast-forward de {} até ela"),
    (
        "The local branch {} is behind {}.",
        "A branch local {} está atrás de {}.",
    ),
    (
        "The local branch {} and {} have diverged: it cannot be fast-forwarded.",
        "A branch local {} e {} divergiram: não é possível fazer fast-forward.",
    ),
    (
        "Check out and fast-forward",
        "Fazer checkout e fast-forward",
    ),
    ("Check out and merge {}", "Fazer checkout e merge de {}"),
    (
        "Check out and rebase onto {}",
        "Fazer checkout e rebase sobre {}",
    ),
    (
        "Check out and reset to {}",
        "Fazer checkout e reset para {}",
    ),
    (
        "Commits only on the local branch will no longer be on it (undo can bring them back).",
        "Os commits que só estão na branch local deixarão de estar nela (desfazer pode trazê-los de volta).",
    ),
    ("Only check out {}", "Somente fazer checkout de {}"),
    (
        "A remote with this name already exists.",
        "Já existe um remoto com este nome.",
    ),
    ("Testing…", "Testando…"),
    ("Test connection", "Testar conexão"),
    ("Save", "Salvar"),
    ("Use “{}”", "Usar “{}”"),
    // ----- Sidebar -----
    ("Fetch {}…", "Fazer fetch de {}…"),
    ("Edit {}…", "Editar {}…"),
    (
        "Fetch {} automatically",
        "Fazer fetch de {} automaticamente",
    ),
    ("Without checking it out", "Sem fazer checkout"),
    ("Change tracking branch…", "Mudar a branch acompanhada…"),
    ("Fast-forward ({})", "Fast-forward ({})"),
    (
        "Fast-forward the selected branches that are behind their upstream",
        "Fazer fast-forward das branches selecionadas que estão atrás do upstream",
    ),
    // ----- Settings -----
    ("Fetch, pull and push", "Fetch, pull e push"),
    (
        "Fetch open repositories every",
        "Fazer fetch dos repositórios abertos a cada",
    ),
    ("(0 turns it off)", "(0 desliga)"),
    (
        "In the background, with --prune. A remote can be left out from its menu in the sidebar.",
        "Em segundo plano, com --prune. Um remoto pode ser deixado de fora pelo menu dele na barra lateral.",
    ),
    (
        "Stash and reapply local changes when pulling",
        "Guardar no stash e reaplicar as alterações locais ao fazer pull",
    ),
    (
        "Push annotated tags with the branch",
        "Fazer push das tags anotadas com a branch",
    ),
];
