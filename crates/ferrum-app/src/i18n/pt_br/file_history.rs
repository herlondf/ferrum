//! Strings of the file history, blame and line history views.

pub static T: &[(&str, &str)] = &[
    // ----- File history -----
    ("File history…", "Histórico do arquivo…"),
    (
        "Type a file or folder name…",
        "Digite o nome de um arquivo ou pasta…",
    ),
    ("No matching file.", "Nenhum arquivo encontrado."),
    ("Open in a window", "Abrir em uma janela"),
    (
        "Show this history in a separate window",
        "Mostrar este histórico em uma janela separada",
    ),
    ("Folders have no blame.", "Pastas não têm blame."),
    ("HEAD (current branch)", "HEAD (branch atual)"),
    (
        "Branch the history is shown for",
        "Branch cujo histórico é mostrado",
    ),
    (
        "No commits touched this folder.",
        "Nenhum commit alterou esta pasta.",
    ),
    (
        "No files in this folder changed.",
        "Nenhum arquivo desta pasta mudou.",
    ),
    (
        "No differences in this file between these commits.",
        "Nenhuma diferença neste arquivo entre estes commits.",
    ),
    ("Comparing {} with {}", "Comparando {} com {}"),
    (
        "(Ctrl+click a commit to change the comparison)",
        "(Ctrl+clique em um commit para mudar a comparação)",
    ),
    (
        "Compare with the selected commit",
        "Comparar com o commit selecionado",
    ),
    ("Open in diff tool", "Abrir na ferramenta de diff"),
    (
        "Compare the two versions in the external diff tool",
        "Comparar as duas versões na ferramenta de diff externa",
    ),
    (
        "Blame of the file at the newer revision",
        "Blame do arquivo na revisão mais nova",
    ),
    ("Blame at this revision", "Blame nesta revisão"),
    (
        "Reset file to this revision…",
        "Restaurar arquivo nesta revisão…",
    ),
    (
        "The file had another name in this commit.",
        "O arquivo tinha outro nome neste commit.",
    ),
    (
        "Replace {} with its version from {}? Uncommitted changes in this file will be lost.",
        "Substituir {} pela versão de {}? Alterações sem commit neste arquivo serão perdidas.",
    ),
    // ----- Blame -----
    ("Back (Alt+Left)", "Voltar (Alt+Esquerda)"),
    ("Forward (Alt+Right)", "Avançar (Alt+Direita)"),
    ("Blame at {}", "Blame em {}"),
    ("Latest", "Mais recente"),
    ("Blame of the newest version", "Blame da versão mais nova"),
    ("(named {} here)", "(chamado {} aqui)"),
    ("Line number", "Número da linha"),
    ("Go to line", "Ir para a linha"),
    ("Go to line…", "Ir para a linha…"),
    (
        "Commits that changed the selected lines",
        "Commits que alteraram as linhas selecionadas",
    ),
    ("History of line {}", "Histórico da linha {}"),
    ("History of lines {}–{}", "Histórico das linhas {}–{}"),
    (
        "Click to blame at this revision",
        "Clique para ver o blame nesta revisão",
    ),
    (
        "Commits of this file: click one to blame at that revision",
        "Commits deste arquivo: clique em um para ver o blame naquela revisão",
    ),
    ("Blame before this change", "Blame antes desta alteração"),
    (
        "Blame at the parent of the commit that last changed this line",
        "Blame no pai do commit que alterou esta linha por último",
    ),
    (
        "The file was added in this commit.",
        "O arquivo foi adicionado neste commit.",
    ),
    ("Blame at this commit", "Blame neste commit"),
    (
        "Select the commit in the file history",
        "Selecionar o commit no histórico do arquivo",
    ),
    ("Copy line", "Copiar linha"),
    // ----- Line history -----
    ("Line {}", "Linha {}"),
    ("Lines {}–{}", "Linhas {}–{}"),
    (
        "History of the selected lines",
        "Histórico das linhas selecionadas",
    ),
    (
        "Commits that changed {} of {} (as of {})",
        "Commits que alteraram {} de {} (a partir de {})",
    ),
    (
        "No commits changed these lines.",
        "Nenhum commit alterou estas linhas.",
    ),
    (
        "Git showed no changes for this commit.",
        "O Git não mostrou alterações neste commit.",
    ),
    ("History of this line", "Histórico desta linha"),
    (
        "These lines are not committed yet.",
        "Estas linhas ainda não têm commit.",
    ),
];
