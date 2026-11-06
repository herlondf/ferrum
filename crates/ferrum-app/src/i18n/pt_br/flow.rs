//! Strings of merge, rebase, cherry-pick, revert and the conflict resolver.

pub static T: &[(&str, &str)] = &[
    // ----- flow.rs: operations and conflict preview -----
    ("Cherry-pick {} commits", "Fazer cherry-pick de {} commits"),
    ("Undo resolution of {}", "Desfazer a resolução de {}"),
    (
        "Undo resolution of {} files",
        "Desfazer a resolução de {} arquivos",
    ),
    (
        "Resolve {} files with ours",
        "Resolver {} arquivos com a nossa versão",
    ),
    (
        "Resolve {} files with theirs",
        "Resolver {} arquivos com a versão deles",
    ),
    ("Checking for conflicts…", "Verificando conflitos…"),
    (
        "✔ Applies cleanly, without conflicts",
        "✔ Aplica sem conflitos",
    ),
    (
        "✔ Already up to date: nothing to do",
        "✔ Já está atualizado: nada a fazer",
    ),
    ("{} more", "mais {}"),
    (
        "⚠ Will conflict at {} in: {}",
        "⚠ Vai gerar conflito em {} nos arquivos: {}",
    ),
    ("⚠ Will conflict in: {}", "⚠ Vai gerar conflito em: {}"),
    (
        "Conflicts could not be checked in advance (needs git 2.38 or newer).",
        "Não foi possível verificar conflitos antes (requer git 2.38 ou mais recente).",
    ),
    // ----- flow_dialogs.rs -----
    ("{} commits", "{} commits"),
    (
        "Squash {} into its parent",
        "Fazer squash de {} no commit pai",
    ),
    ("Squash {} commits", "Fazer squash de {} commits"),
    (
        "Fix up {} into its parent",
        "Fazer fixup de {} no commit pai",
    ),
    ("Edit {}", "Editar {}"),
    ("Drop {}", "Descartar {}"),
    ("Drop {} commits", "Descartar {} commits"),
    ("Reword {}", "Reescrever a mensagem de {}"),
    ("No commit selected.", "Nenhum commit selecionado."),
    (
        "Merge commits cannot be changed this way.",
        "Commits de merge não podem ser alterados assim.",
    ),
    ("There is no commit yet.", "Ainda não há nenhum commit."),
    (
        "The commit is not on the current branch.",
        "O commit não está no branch atual.",
    ),
    (
        "The commits are not on one line of history.",
        "Os commits não estão numa mesma linha do histórico.",
    ),
    (
        "The first commit has no parent to meld into.",
        "O primeiro commit não tem um commit pai para receber a fusão.",
    ),
    (
        "There are merge commits after it: use the interactive rebase instead.",
        "Há commits de merge depois dele: use o rebase interativo.",
    ),
    ("Merge {} into {}", "Fazer merge de {} em {}"),
    (
        "Don't commit (--no-commit)",
        "Não fazer commit (--no-commit)",
    ),
    (
        "Merge the changes but stop before committing, to review or edit them",
        "Aplica o merge mas para antes do commit, para revisar ou editar as alterações",
    ),
    (
        "Allow unrelated histories",
        "Permitir históricos não relacionados",
    ),
    (
        "Merge a branch that shares no commit with this one",
        "Fazer merge de um branch que não tem nenhum commit em comum com este",
    ),
    ("Rebase {}", "Fazer rebase de {}"),
    ("Onto", "Sobre"),
    (
        "Stash and reapply local changes (--autostash)",
        "Guardar no stash e reaplicar as alterações locais (--autostash)",
    ),
    (
        "Update dependent branches (--update-refs)",
        "Atualizar branches dependentes (--update-refs)",
    ),
    (
        "Move other branches that point at the rebased commits along with them",
        "Move junto os outros branches que apontam para os commits do rebase",
    ),
    ("Dependent branches: {}", "Branches dependentes: {}"),
    ("Cherry-pick onto {}", "Cherry-pick sobre {}"),
    ("Commit the changes", "Fazer commit das alterações"),
    (
        "Append the origin to the message (-x)",
        "Acrescentar a origem à mensagem (-x)",
    ),
    (
        "Adds \"(cherry picked from commit …)\" to the commit message",
        "Acrescenta \"(cherry picked from commit …)\" à mensagem do commit",
    ),
    ("Cherry-pick", "Cherry-pick"),
    ("Revert on {}", "Reverter em {}"),
    ("Revert", "Reverter"),
    (
        "The commits after it will be rewritten.",
        "Os commits depois dele serão reescritos.",
    ),
    ("Reword", "Reescrever"),
    (
        "There are local changes in 1 file.",
        "Há alterações locais em 1 arquivo.",
    ),
    (
        "There are local changes in {} files.",
        "Há alterações locais em {} arquivos.",
    ),
    (
        "These local changes would be overwritten:",
        "Estas alterações locais seriam sobrescritas:",
    ),
    (
        "Stash and reapply them afterwards",
        "Guardar no stash e reaplicar depois",
    ),
    ("Discard them", "Descartá-las"),
    ("Keep them as they are", "Mantê-las como estão"),
    ("Parent {}: {}", "Pai {}: {}"),
    ("Parent {}", "Pai {}"),
    ("Mainline", "Linha principal"),
    (
        "This is a merge commit: changes are taken relative to this parent",
        "Este é um commit de merge: as alterações são calculadas em relação a este pai",
    ),
    // ----- banner.rs -----
    ("Show this commit", "Mostrar este commit"),
    ("Merging", "Fazendo merge de"),
    ("into", "em"),
    ("Rebasing", "Fazendo rebase de"),
    ("onto", "sobre"),
    ("({} of {})", "({} de {})"),
    ("stopped at", "parado em"),
    ("Cherry-picking", "Fazendo cherry-pick de"),
    ("Reverting", "Revertendo"),
    ("on", "em"),
    (
        "fix 1 conflict and then continue.",
        "resolva 1 conflito e continue.",
    ),
    (
        "fix {} conflicts and then continue.",
        "resolva {} conflitos e continue.",
    ),
    (
        "no conflicts left: continue when ready.",
        "não há mais conflitos: continue quando quiser.",
    ),
    (
        "Leave out the commit being applied",
        "Pular o commit que está sendo aplicado",
    ),
    ("Resolved ({}) ⏷", "Resolvidos ({}) ⏷"),
    (
        "Bring back the conflict of a file marked resolved:",
        "Trazer de volta o conflito de um arquivo marcado como resolvido:",
    ),
    ("Undo {}", "Desfazer {}"),
    ("Undo all", "Desfazer todos"),
    // ----- conflicts.rs -----
    ("Use {} for whole file", "Usar {} no arquivo todo"),
    (
        "All conflicted files ({})",
        "Todos os arquivos com conflito ({})",
    ),
    ("Select all", "Selecionar todos"),
    ("Select none", "Limpar seleção"),
    ("Use {} for {} selected", "Usar {} nos {} selecionados"),
    ("Use {}", "Usar {}"),
    (
        "Both sides changed this image. Keep one of them:",
        "Os dois lados alteraram esta imagem. Mantenha um deles:",
    ),
    ("Left: {} · Right: {}", "Esquerda: {} · Direita: {}"),
    (
        "Both sides moved the submodule {} to different commits.",
        "Os dois lados moveram o submódulo {} para commits diferentes.",
    ),
    ("Pick the commit to keep:", "Escolha o commit a manter:"),
    ("(deleted)", "(excluído)"),
    (
        "Only the recorded commit changes: update the submodule afterwards to check it out.",
        "Só o commit registrado muda: atualize o submódulo depois para fazer checkout dele.",
    ),
    (
        "This file is stored with Git LFS, so it cannot be merged line by line.",
        "Este arquivo é guardado com Git LFS, então não pode ser mesclado linha a linha.",
    ),
    ("Changes compared with", "Alterações em relação a"),
    ("The result is the same as {}.", "O resultado é igual a {}."),
    ("Previous conflict", "Conflito anterior"),
    ("Next conflict", "Próximo conflito"),
    ("Both ({} first)", "Ambas ({} primeiro)"),
    // ----- history_flow.rs -----
    (
        "{}? The commits of {} will be rewritten.",
        "{}? Os commits de {} serão reescritos.",
    ),
    ("{} commits selected", "{} commits selecionados"),
    (
        "Cherry-pick {} commits…",
        "Fazer cherry-pick de {} commits…",
    ),
    (
        "Squash {} commits into one…",
        "Juntar {} commits em um (squash)…",
    ),
    (
        "Only commits of the current branch can be squashed",
        "Só commits do branch atual podem ser juntados",
    ),
    ("Drop {} commits…", "Descartar {} commits…"),
    (
        "Only commits of the current branch can be dropped",
        "Só commits do branch atual podem ser descartados",
    ),
    ("Clear selection", "Limpar seleção"),
    (
        "Interactive rebase from {}…",
        "Rebase interativo a partir de {}…",
    ),
    ("Quick actions", "Ações rápidas"),
    ("Reword…", "Reescrever a mensagem…"),
    ("Edit…", "Editar…"),
    (
        "Stop the rebase at this commit so you can amend it",
        "Parar o rebase neste commit para você poder corrigi-lo",
    ),
    ("Squash into parent…", "Squash no commit pai…"),
    (
        "Meld into the commit before it, keeping both messages",
        "Fundir no commit anterior, mantendo as duas mensagens",
    ),
    ("Fixup into parent…", "Fixup no commit pai…"),
    (
        "Meld into the commit before it, keeping its message",
        "Fundir no commit anterior, mantendo a mensagem dele",
    ),
    ("Drop…", "Descartar…"),
    ("Cherry-pick {}…", "Fazer cherry-pick de {}…"),
    ("Revert {}…", "Reverter {}…"),
    // ----- rebase.rs: planner -----
    (
        "Click to select (Ctrl/Shift for several), then press P R E S F D to pick, reword, edit, squash, fix up or drop, and Alt+⬆ ⬇ to move.",
        "Clique para selecionar (Ctrl/Shift para vários), depois tecle P R E S F D para pick, reword, edit, squash, fixup ou drop, e Alt+⬆ ⬇ para mover.",
    ),
    (
        "Back up the current state in a branch first",
        "Guardar antes o estado atual num branch de backup",
    ),
    (
        "Creates a backup/… branch at the current commit, to get back to it later",
        "Cria um branch backup/… no commit atual, para poder voltar a ele depois",
    ),
    (
        "Update dependent branches: {}",
        "Atualizar branches dependentes: {}",
    ),
    (
        "Discard the changes to this plan?",
        "Descartar as alterações deste plano?",
    ),
    ("Discard", "Descartar"),
    ("Keep editing", "Continuar editando"),
    // ----- preferences.rs -----
    ("Local changes on checkout", "Alterações locais no checkout"),
    (
        "Always ask to stash or discard them",
        "Sempre perguntar se devem ir para o stash ou ser descartadas",
    ),
    (
        "Otherwise Ferrum only asks when the checkout would overwrite them",
        "Senão o Ferrum só pergunta quando o checkout as sobrescreveria",
    ),
];
