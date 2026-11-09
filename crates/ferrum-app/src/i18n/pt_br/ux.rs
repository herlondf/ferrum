//! Strings of drag and drop, the start screen, the commit area and the
//! command palette.

pub static T: &[(&str, &str)] = &[
    // ----- Drag and drop of branches -----
    (
        "Cherry-pick {} onto {}…",
        "Fazer cherry-pick de {} sobre {}…",
    ),
    ("Cherry-pick {} onto {}", "Fazer cherry-pick de {} sobre {}"),
    ("Push {} to {}…", "Fazer push de {} para {}…"),
    (
        "Check out {} and copy {} commits of {} onto it?",
        "Fazer checkout de {} e copiar para ela {} commits de {}?",
    ),
    (
        "{} has no commits that {} lacks.",
        "{} não tem commits que faltem em {}.",
    ),
    (
        "{} has more than {} commits that {} lacks: merge or rebase instead.",
        "{} tem mais de {} commits que faltam em {}: use merge ou rebase.",
    ),
    // ----- Local changes -----
    ("Unstaged", "Não preparadas"),
    ("Staged", "Preparadas"),
    ("Staged + last commit", "Preparadas + último commit"),
    ("Drop to stage", "Solte para preparar"),
    ("Drop to stage {} files", "Solte para preparar {} arquivos"),
    ("Drop to unstage", "Solte para remover da preparação"),
    (
        "Drop to unstage {} files",
        "Solte para remover {} arquivos da preparação",
    ),
    ("Added", "Adicionado"),
    ("Modified", "Modificado"),
    ("Deleted", "Excluído"),
    ("Renamed", "Renomeado"),
    ("Copied", "Copiado"),
    ("New file", "Arquivo novo"),
    ("Conflicted", "Em conflito"),
    ("Type changed", "Tipo alterado"),
    // ----- Commit area -----
    ("More ways to commit", "Outras formas de fazer commit"),
    (
        "Stage and commit 1 file",
        "Preparar e fazer commit de 1 arquivo",
    ),
    (
        "Stage and commit {} files",
        "Preparar e fazer commit de {} arquivos",
    ),
    (
        "Stage every change, then commit",
        "Preparar todas as alterações e fazer commit",
    ),
    (
        "Resolve the conflicts first",
        "Resolva os conflitos primeiro",
    ),
    ("The commit goes on {}", "O commit vai para {}"),
    ("detached HEAD", "HEAD desanexado"),
    // ----- Start screen -----
    ("New", "Novo"),
    (
        "A repository on this computer",
        "Um repositório neste computador",
    ),
    (
        "Copy a repository from a URL",
        "Copiar um repositório de uma URL",
    ),
    ("Start an empty repository", "Criar um repositório vazio"),
    ("Drop the folder to open it", "Solte a pasta para abri-la"),
    (
        "Found in source folders",
        "Encontrados nas pastas de origem",
    ),
    ("No repositories yet", "Nenhum repositório ainda"),
    (
        "No repository matches “{}”.",
        "Nenhum repositório corresponde a “{}”.",
    ),
    (
        "Open, clone or create one above, or drop a project folder anywhere on this window.",
        "Abra, clone ou crie um acima, ou solte a pasta de um projeto em qualquer lugar desta janela.",
    ),
    (
        "Tip: add a source folder on the left to list all your repositories.",
        "Dica: adicione uma pasta de origem à esquerda para listar todos os seus repositórios.",
    ),
    ("open", "aberto"),
    ("missing", "ausente"),
    // ----- Command palette -----
    ("Recently used", "Usados recentemente"),
    ("Show {}", "Mostrar {}"),
    ("move", "mover"),
    ("run", "executar"),
    ("close", "fechar"),
];
