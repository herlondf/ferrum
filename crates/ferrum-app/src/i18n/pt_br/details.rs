//! Strings of the commit details, file tree and compare view.

pub static T: &[(&str, &str)] = &[
    // ----- Details -----
    ("Changes", "Alterações"),
    ("File Tree", "Árvore de arquivos"),
    ("Copy SHA", "Copiar SHA"),
    ("Copy info", "Copiar informações"),
    ("Click to select it", "Clique para selecioná-lo"),
    ("Searching…", "Procurando…"),
    ("Open in new window", "Abrir em nova janela"),
    (
        "Show this commit in its own window",
        "Mostrar este commit em uma janela própria",
    ),
    ("Large view", "Visão ampliada"),
    ("Space in the commit list", "Espaço na lista de commits"),
    (
        "Compare with working copy",
        "Comparar com a cópia de trabalho",
    ),
    // ----- File lists and file tree -----
    ("List", "Lista"),
    ("Expand all", "Expandir tudo"),
    ("Collapse all", "Recolher tudo"),
    ("Files at {}", "Arquivos em {}"),
    ("No files.", "Nenhum arquivo."),
    ("Save file…", "Salvar arquivo…"),
    (
        "Select a file to see its content.",
        "Selecione um arquivo para ver o conteúdo.",
    ),
    (
        "{} does not exist in this commit.",
        "{} não existe neste commit.",
    ),
    ("Binary file ({}).", "Arquivo binário ({})."),
    (
        "This file is too large to show ({}). Save it to open it in another program.",
        "Este arquivo é grande demais para exibir ({}). Salve-o para abri-lo em outro programa.",
    ),
    ("Submodule at commit {}", "Submódulo no commit {}"),
    // ----- External tools -----
    ("External diff", "Diff externo"),
    (
        "The file is not in the working copy.",
        "O arquivo não está na cópia de trabalho.",
    ),
    ("Open in editor at line {}", "Abrir no editor na linha {}"),
    ("Copy line", "Copiar linha"),
    // ----- Compare -----
    ("Compare", "Comparar"),
    ("Working copy", "Cópia de trabalho"),
    ("🔃 Swap", "🔃 Inverter"),
    (
        "Swap the order of the two sides",
        "Inverter a ordem dos dois lados",
    ),
    ("No differences.", "Nenhuma diferença."),
    (
        "Show the changes between {} and {}",
        "Mostrar as alterações entre {} e {}",
    ),
];
