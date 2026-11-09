//! Strings of the history, details, diff, blame and overview polish (#20).

pub static T: &[(&str, &str)] = &[
    // ----- Commit list -----
    ("Relative dates (3 h ago)", "Datas relativas (há 3 h)"),
    ("{} min ago", "há {} min"),
    ("{} h ago", "há {} h"),
    ("{} weeks ago", "há {} semanas"),
    ("{} months ago", "há {} meses"),
    ("1 year ago", "há 1 ano"),
    ("{} years ago", "há {} anos"),
    // ----- Empty states -----
    ("No commit selected", "Nenhum commit selecionado"),
    ("Move between commits", "Navegar entre os commits"),
    ("Move between files", "Navegar entre os arquivos"),
    ("No file selected", "Nenhum arquivo selecionado"),
    (
        "Pick a file in the list to see its changes.",
        "Escolha um arquivo na lista para ver as alterações.",
    ),
    (
        "The working tree matches the last commit.",
        "A cópia de trabalho está igual ao último commit.",
    ),
    ("Clear search", "Limpar pesquisa"),
    (
        "Try other words, or search in the changes or file paths.",
        "Tente outras palavras, ou pesquise nas alterações ou nos caminhos dos arquivos.",
    ),
    (
        "Your first commit will show up here.",
        "Seu primeiro commit vai aparecer aqui.",
    ),
    // ----- Diff -----
    ("Unified", "Unificado"),
    ("Context", "Contexto"),
    ("Size", "Tamanho"),
    // ----- Blame -----
    ("Line age", "Idade das linhas"),
    (
        "The gutter is colored by the age of each line: newer lines are stronger.",
        "A margem é colorida pela idade de cada linha: as mais novas são mais fortes.",
    ),
    // ----- Overview -----
    (
        "Add a README.md to describe the project; it will show up here.",
        "Adicione um README.md para descrever o projeto; ele vai aparecer aqui.",
    ),
    (
        "The image is outside the repository.",
        "A imagem está fora do repositório.",
    ),
    ("last 12 months", "últimos 12 meses"),
    ("Week of {}: 1 commit", "Semana de {}: 1 commit"),
    ("Week of {}: {} commits", "Semana de {}: {} commits"),
    (
        "No commits in the last 12 months.",
        "Nenhum commit nos últimos 12 meses.",
    ),
    (
        "1 commit in the last 12 months",
        "1 commit nos últimos 12 meses",
    ),
    (
        "{} commits in the last 12 months",
        "{} commits nos últimos 12 meses",
    ),
];
