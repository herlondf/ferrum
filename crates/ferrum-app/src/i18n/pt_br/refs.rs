//! Strings of the branch, tag and stash dialogs.

pub static T: &[(&str, &str)] = &[
    // ----- Branches -----
    ("Check out branch", "Fazer checkout de branch"),
    ("Check out branch…", "Fazer checkout de branch…"),
    ("Search branches", "Pesquisar branches"),
    (
        "☁ creates a local branch that tracks the remote one.",
        "☁ cria uma branch local que acompanha a remota.",
    ),
    ("Folders:", "Pastas:"),
    (
        "A branch with this name already exists.",
        "Já existe uma branch com este nome.",
    ),
    (
        "Git does not accept this branch name.",
        "O Git não aceita este nome de branch.",
    ),
    ("Also rename {} on {}", "Renomear também {} em {}"),
    (
        "Pushes the new name, deletes the old one and tracks the new one",
        "Envia o novo nome, exclui o antigo e passa a acompanhar o novo",
    ),
    ("Rename…  (F2)", "Renomear…  (F2)"),
    ("checked out in {}", "em checkout em {}"),
    (
        "Also delete their remote branches ({})",
        "Excluir também as branches remotas ({})",
    ),
    (
        "Also remove the worktrees that have them checked out ({})",
        "Remover também as worktrees que estão com elas em checkout ({})",
    ),
    (
        "A branch checked out in another worktree cannot be deleted.",
        "Uma branch em checkout em outra worktree não pode ser excluída.",
    ),
    ("Copy branch name", "Copiar nome da branch"),
    // ----- Tags -----
    (
        "A tag with this name already exists.",
        "Já existe uma tag com este nome.",
    ),
    ("Sign the tag (-s)", "Assinar a tag (-s)"),
    ("With the signing key {}", "Com a chave de assinatura {}"),
    ("Tag details", "Detalhes da tag"),
    ("Tag {} details…", "Detalhes da tag {}…"),
    ("Details…", "Detalhes…"),
    ("Show in the history", "Mostrar no histórico"),
    ("Points to", "Aponta para"),
    ("Type", "Tipo"),
    ("Annotated", "Anotada"),
    ("Annotated, signed", "Anotada, assinada"),
    ("Lightweight (no message)", "Leve (sem mensagem)"),
    ("Tagger", "Autor da tag"),
    ("Delete remote tag", "Excluir tag remota"),
    (
        "Delete the tag {} from {}? The local tag is kept.",
        "Excluir a tag {} de {}? A tag local é mantida.",
    ),
    // ----- Stashes -----
    ("Stashes", "Stashes"),
    ("Quick stash", "Stash rápido"),
    (
        "Stash all changes, new files included, without asking",
        "Guardar todas as alterações no stash, inclusive arquivos novos, sem perguntar",
    ),
    (
        "Stash changes (Ctrl+Shift+S)\nCtrl/Alt+click: quick stash without the dialog",
        "Guardar alterações no stash (Ctrl+Shift+S)\nCtrl/Alt+clique: stash rápido sem a janela",
    ),
    ("Save a snapshot", "Salvar um snapshot"),
    (
        "Save the changes as a stash and keep them in the working copy",
        "Salvar as alterações como stash e mantê-las na cópia de trabalho",
    ),
    (
        "Keep the changes in the working copy (snapshot)",
        "Manter as alterações na cópia de trabalho (snapshot)",
    ),
    (
        "New (untracked) files are not saved in a snapshot.",
        "Arquivos novos (não rastreados) não são salvos em um snapshot.",
    ),
    ("Only some files", "Somente alguns arquivos"),
    ("{} of {} files", "{} de {} arquivos"),
    ("None", "Nenhum"),
    ("Stash {}", "Stash de {}"),
    (
        "There are no changes to save.",
        "Não há alterações para salvar.",
    ),
    ("Apply with options…", "Aplicar com opções…"),
    (
        "Restore the staged state (--index)",
        "Restaurar o estado do stage (--index)",
    ),
    (
        "Files that were staged when stashing are staged again",
        "Arquivos que estavam no stage ao guardar voltam para o stage",
    ),
    (
        "Delete the stash after applying it (pop)",
        "Excluir o stash depois de aplicar (pop)",
    ),
    (
        "The stash moves to the top of the list.",
        "O stash vai para o topo da lista.",
    ),
    ("Save {} as a patch", "Salvar {} como patch"),
    ("Show changes", "Mostrar alterações"),
    ("Drop several stashes…", "Descartar vários stashes…"),
    ("Drop stashes", "Descartar stashes"),
    ("Drop {} stashes", "Descartar {} stashes"),
    (
        "Dropped stashes cannot be restored.",
        "Stashes descartados não podem ser restaurados.",
    ),
];
