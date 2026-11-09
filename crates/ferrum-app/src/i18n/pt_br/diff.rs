//! Strings of the diff viewer.

pub static T: &[(&str, &str)] = &[
    // ----- Toolbar -----
    ("Side by side", "Lado a lado"),
    ("Wrap", "Quebrar linhas"),
    ("Wrap long lines", "Quebra as linhas longas"),
    ("Ignore whitespace", "Ignorar espaços"),
    (
        "Hide changes that only touch whitespace",
        "Oculta alterações que só mexem em espaços em branco",
    ),
    ("Entire file", "Arquivo inteiro"),
    (
        "Show the whole file, not just the changed parts",
        "Mostra o arquivo todo, não só as partes alteradas",
    ),
    ("{} lines", "{} linhas"),
    (
        "Unchanged lines shown around each change",
        "Linhas inalteradas mostradas em volta de cada alteração",
    ),
    ("Smaller text", "Texto menor"),
    ("Larger text", "Texto maior"),
    ("Previous change", "Alteração anterior"),
    ("Next change", "Próxima alteração"),
    ("Find in diff (Ctrl+F)", "Buscar no diff (Ctrl+F)"),
    ("Show whitespace", "Mostrar espaços em branco"),
    ("Tab width", "Largura da tabulação"),
    ("Show -/+ markers", "Mostrar marcadores -/+"),
    ("Syntax highlighting", "Realce de sintaxe"),
    ("Highlight changed words", "Destacar palavras alteradas"),
    ("Map of changes", "Mapa de alterações"),
    ("Diff options", "Opções do diff"),
    (
        "Open in a large window (Space)",
        "Abrir em uma janela grande (Espaço)",
    ),
    (
        "Staging lines and hunks is off while whitespace is ignored.",
        "Preparar linhas e blocos fica desativado enquanto os espaços são ignorados.",
    ),
    // ----- File info -----
    ("Renamed: {} -> {}", "Renomeado: {} -> {}"),
    ("Mode changed: {} -> {}", "Modo alterado: {} -> {}"),
    ("New file with mode {}", "Novo arquivo com modo {}"),
    ("executable", "executável"),
    ("symbolic link", "link simbólico"),
    ("submodule", "submódulo"),
    ("regular file", "arquivo comum"),
    // ----- Find -----
    ("Find in diff", "Buscar no diff"),
    (
        "Previous match (Shift+Enter)",
        "Resultado anterior (Shift+Enter)",
    ),
    ("Next match (Enter)", "Próximo resultado (Enter)"),
    ("No matches", "Nenhum resultado"),
    (
        "Map of changes: click to jump",
        "Mapa de alterações: clique para ir até o ponto",
    ),
    // ----- Large window, large files, binaries -----
    (
        "Space or Esc closes this window",
        "Espaço ou Esc fecha esta janela",
    ),
    (
        "This untracked file is large ({}), so its diff was not loaded.",
        "Este arquivo não rastreado é grande ({}), então o diff não foi carregado.",
    ),
    ("Load diff", "Carregar diff"),
    ("{} bytes", "{} bytes"),
    ("{} bytes -> {} bytes", "{} bytes -> {} bytes"),
    ("Size change: {}{}", "Diferença de tamanho: {}{}"),
    ("Text", "Texto"),
    ("Image", "Imagem"),
    // ----- Submodules -----
    ("Submodule {}: {} -> {}", "Submódulo {}: {} -> {}"),
    (
        "No commits between the two revisions.",
        "Nenhum commit entre as duas revisões.",
    ),
    ("Commits added ({})", "Commits adicionados ({})"),
    ("Commits removed ({})", "Commits removidos ({})"),
    (
        "Only the first commits are listed.",
        "Só os primeiros commits são listados.",
    ),
    // ----- Git LFS -----
    ("Git LFS file", "Arquivo do Git LFS"),
    (
        "Install Git LFS to download and preview the content.",
        "Instale o Git LFS para baixar e visualizar o conteúdo.",
    ),
    ("Download / preview", "Baixar / visualizar"),
    (
        "Fetch the content with git lfs (may download)",
        "Busca o conteúdo com o git lfs (pode baixar)",
    ),
    ("Downloading…", "Baixando…"),
    ("git lfs failed: {}", "git lfs falhou: {}"),
    ("Binary content ({}).", "Conteúdo binário ({})."),
    // ----- Image diff -----
    ("Swipe", "Deslizar"),
    ("Onion skin", "Sobreposição"),
    ("Difference", "Diferença"),
    ("Drag to move the divider", "Arraste para mover a divisória"),
    (
        "The pixel difference needs both images at the same size.",
        "A diferença de pixels precisa das duas imagens no mesmo tamanho.",
    ),
    (
        "Image too large to compare pixel by pixel.",
        "Imagem grande demais para comparar pixel a pixel.",
    ),
    (
        "The images are identical pixel by pixel.",
        "As imagens são idênticas pixel a pixel.",
    ),
    ("{} pixels differ ({}%)", "{} pixels diferentes ({}%)"),
];
