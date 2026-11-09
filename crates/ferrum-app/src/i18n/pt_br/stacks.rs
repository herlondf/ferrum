//! Strings of stacked branches, absorb and moving hunks between commits.

pub static T: &[(&str, &str)] = &[
    // ----- Operations and toasts -----
    ("Restack {}", "Reempilhar {}"),
    ("Create {} {}s for the stack", "Criar {} {}s para a pilha"),
    ("Absorb local changes", "Absorver alterações locais"),
    ("Move {} to {}", "Mover {} para {}"),
    ("Restore {}", "Restaurar {}"),
    (
        "Every branch of the stack already has a {}.",
        "Todas as branches da pilha já têm um {}.",
    ),
    ("Created {} {}s: {}", "{} {}s criados: {}"),
    ("Restacked {} on {}", "{} reempilhadas sobre {}"),
    ("Absorbed {} into {}", "{} absorvidas em {}"),
    (
        "Created {} fixup commits for {}",
        "{} commits de fixup criados para {}",
    ),
    ("Moved {} to {}", "Movido: {} para {}"),
    ("1 branch", "1 branch"),
    ("{} branches", "{} branches"),
    ("1 change", "1 alteração"),
    ("{} changes", "{} alterações"),
    (
        "Stopped for conflicts: resolve them, then continue or abort.",
        "Parou por conflitos: resolva-os e depois continue ou aborte.",
    ),
    // ----- Restack, push and pull requests -----
    (
        "The branches are saved first as a backup (refs/ferrum/backup/…): Undo puts them back.",
        "Antes, as branches são salvas como backup (refs/ferrum/backup/…): Desfazer as traz de volta.",
    ),
    (
        "Every branch is based on the latest commit of the one below it: nothing to restack.",
        "Cada branch já parte do último commit da branch abaixo: nada a reempilhar.",
    ),
    (
        "These branches are rebased onto the branch below them, bottom first:",
        "Estas branches passam por rebase sobre a branch abaixo delas, de baixo para cima:",
    ),
    ("{} onto {}", "{} sobre {}"),
    ("base", "base"),
    (
        "One rebase moves the whole stack (--update-refs). If it stops for a conflict, resolve it and continue, or abort to leave everything as it was.",
        "Um único rebase move a pilha inteira (--update-refs). Se parar por um conflito, resolva e continue, ou aborte para deixar tudo como estava.",
    ),
    (
        "This git is older than 2.38, so the branches are rebased one at a time. After a conflict, continue the rebase and restack again.",
        "Este git é anterior ao 2.38, então as branches passam por rebase uma de cada vez. Depois de um conflito, continue o rebase e reempilhe de novo.",
    ),
    (
        "Commit or stash the local changes first.",
        "Faça commit ou stash das alterações locais primeiro.",
    ),
    ("Restack", "Reempilhar"),
    ("Push stack {}", "Enviar a pilha {}"),
    (
        "This repository has no remote.",
        "Este repositório não tem remoto.",
    ),
    ("To", "Para"),
    ("not pushed yet", "ainda não enviada"),
    ("up to date", "atualizada"),
    ("{} to push", "{} a enviar"),
    ("{} ahead, {} behind", "{} à frente, {} atrás"),
    (
        "Force with lease (needed after a restack)",
        "Forçar com lease (necessário depois de reempilhar)",
    ),
    (
        "Overwrites the remote branches only if nobody else pushed to them since they were fetched",
        "Sobrescreve as branches remotas só se ninguém mais enviou para elas desde o último fetch",
    ),
    ("Pull requests for the stack", "Pull requests para a pilha"),
    (
        "No remote of this repository is on a hosting service Ferrum knows.",
        "Nenhum remoto deste repositório está num serviço de hospedagem que o Ferrum conhece.",
    ),
    ("Create {}s for stack {}", "Criar {}s para a pilha {}"),
    (
        "One {} per branch, each based on the branch below it. Every description links the others.",
        "Um {} por branch, cada um baseado na branch abaixo. Cada descrição tem links para os outros.",
    ),
    (
        "Description (added to each)",
        "Descrição (adicionada a cada um)",
    ),
    ("Drafts", "Rascunhos"),
    ("Push the stack first", "Enviar a pilha antes"),
    ("Log in to {} first.", "Entre no {} primeiro."),
    ("Create {}s", "Criar {}s"),
    // ----- New branch in a stack -----
    ("New branch on top of {}", "Nova branch sobre {}"),
    (
        "It starts at the last commit of {} and is stacked on it.",
        "Ela começa no último commit de {} e fica empilhada sobre ela.",
    ),
    ("New branch below {}", "Nova branch abaixo de {}"),
    (
        "It goes between {} and {}. Commit on it, then restack to move {} on top.",
        "Ela fica entre {} e {}. Faça commits nela e depois reempilhe para mover {} para cima.",
    ),
    (
        "A branch named {} already exists.",
        "Já existe uma branch chamada {}.",
    ),
    ("Check it out", "Fazer checkout dela"),
    (
        "The local changes come along to the new branch.",
        "As alterações locais vão junto para a nova branch.",
    ),
    // ----- Sidebar, menus and palette -----
    ("Stacks", "Pilhas"),
    (
        "Branches based on each other: restack, push or open pull requests for all of them at once",
        "Branches baseadas umas nas outras: reempilhe, envie ou abra pull requests para todas de uma vez",
    ),
    ("Stack of {} on {}", "Pilha de {} sobre {}"),
    ("Restack needed…", "Precisa reempilhar…"),
    (
        "A branch below has new commits",
        "Uma branch abaixo tem commits novos",
    ),
    (
        "{} is not based on the latest commit of {}: restack it",
        "{} não parte do último commit de {}: reempilhe",
    ),
    ("{} on {}", "{} sobre {}"),
    ("The base of the stack", "A base da pilha"),
    ("Restack…", "Reempilhar…"),
    ("Push stack…", "Enviar a pilha…"),
    ("Create {}s for the stack…", "Criar {}s para a pilha…"),
    ("New branch on top of {}…", "Nova branch sobre {}…"),
    (
        "Insert a branch below {}…",
        "Inserir uma branch abaixo de {}…",
    ),
    ("Stack", "Pilha"),
    (
        "Stack a new branch on top…",
        "Empilhar uma nova branch por cima…",
    ),
    (
        "Absorb local changes into commits…",
        "Absorver alterações locais nos commits…",
    ),
    ("Restack {}…", "Reempilhar {}…"),
    ("Absorb into commits…", "Absorver nos commits…"),
    // ----- Absorb -----
    (
        "new file: no commit touched it yet",
        "arquivo novo: nenhum commit mexeu nele ainda",
    ),
    (
        "deleted, renamed, binary or mode change",
        "excluído, renomeado, binário ou mudança de modo",
    ),
    (
        "new lines next to lines of other commits",
        "linhas novas ao lado de linhas de outros commits",
    ),
    (
        "the lines come from a commit that is not on this branch",
        "as linhas vêm de um commit que não está nesta branch",
    ),
    (
        "the lines come from several commits",
        "as linhas vêm de vários commits",
    ),
    (
        "Absorb local changes into commits",
        "Absorver alterações locais nos commits",
    ),
    (
        "Finding the commit that last touched each change…",
        "Procurando o último commit que mexeu em cada alteração…",
    ),
    ("Absorb", "Absorver"),
    (
        "Each staged change goes into the commit of {} that last touched its lines, as a fixup commit.",
        "Cada alteração preparada vai para o commit de {} que mexeu por último nas suas linhas, como um commit de fixup.",
    ),
    (
        "Each change goes into the commit of {} that last touched its lines, as a fixup commit.",
        "Cada alteração vai para o commit de {} que mexeu por último nas suas linhas, como um commit de fixup.",
    ),
    (
        "There are no local changes to absorb.",
        "Não há alterações locais para absorver.",
    ),
    (
        "None of the changes can be absorbed: they stay as local changes.",
        "Nenhuma alteração pode ser absorvida: elas continuam como alterações locais.",
    ),
    (
        "Meld the fixup commits in right away (autosquash rebase)",
        "Juntar os commits de fixup na hora (rebase com autosquash)",
    ),
    (
        "Otherwise the fixup! commits stay on top, to squash later with an interactive rebase",
        "Senão os commits fixup! ficam no topo, para o squash depois num rebase interativo",
    ),
    (
        "Branches on these commits move along: {}",
        "As branches nesses commits vão junto: {}",
    ),
    (
        "This git is older than 2.38: {} will not move along.",
        "Este git é anterior ao 2.38: {} não vai junto.",
    ),
    (
        "If the rebase stops for a conflict, resolve it and continue, or abort it.",
        "Se o rebase parar por um conflito, resolva e continue, ou aborte.",
    ),
    (
        "Check out a branch first.",
        "Faça checkout de uma branch primeiro.",
    ),
    ("Absorb {}", "Absorver {}"),
    ("Stays as a local change", "Continua como alteração local"),
    ("{}, after line {}", "{}, depois da linha {}"),
    ("{}, line {}", "{}, linha {}"),
    ("{}, lines {}–{}", "{}, linhas {}–{}"),
    ("{} more lines", "mais {} linhas"),
    // ----- Moving hunks -----
    ("1 line of {}", "1 linha de {}"),
    ("{} lines of {}", "{} linhas de {}"),
    (
        "Drag onto a commit in the history to move it there",
        "Arraste para um commit do histórico para mover para lá",
    ),
    ("Move {}", "Mover {}"),
    ("to…", "para…"),
    (
        "A new commit right after it…",
        "Um commit novo logo depois dele…",
    ),
    (
        "Into a commit of this branch",
        "Para um commit desta branch",
    ),
    ("Select", "Selecionar"),
    ("All changes", "Todas as alterações"),
    ("Hunk {}: lines {}–{}", "Trecho {}: linhas {}–{}"),
    (
        "Or click lines in the diff, Shift+click for a range",
        "Ou clique nas linhas do diff, Shift+clique para um intervalo",
    ),
    (
        "Turn off “Ignore whitespace” to move single lines",
        "Desligue “Ignorar espaços em branco” para mover linhas soltas",
    ),
    ("Split into a new commit", "Separar num commit novo"),
    ("Move to another commit", "Mover para outro commit"),
    (
        "a new commit right after it",
        "um commit novo logo depois dele",
    ),
    (
        "One interactive rebase rewrites the commits from there up. If it stops for a conflict, resolve it and continue, or abort to leave everything as it was.",
        "Um rebase interativo reescreve os commits dali para cima. Se parar por um conflito, resolva e continue, ou aborte para deixar tudo como estava.",
    ),
    (
        "Local changes are stashed during the rebase and put back afterwards.",
        "As alterações locais vão para o stash durante o rebase e voltam depois.",
    ),
    ("Split", "Separar"),
    ("a new commit", "um commit novo"),
];
