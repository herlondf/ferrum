//! Strings of the Repository Manager, tabs, workspaces and overview.

pub static T: &[(&str, &str)] = &[
    // ----- Repository Manager -----
    ("Repositories", "Repositórios"),
    ("📁 Repositories", "📁 Repositórios"),
    ("Repository Manager", "Gerenciador de repositórios"),
    (
        "Repository Manager  (Ctrl+T)",
        "Gerenciador de repositórios  (Ctrl+T)",
    ),
    ("Filter repositories", "Filtrar repositórios"),
    (
        "Drop a folder on the window to open it. Enter opens, F2 renames, Delete removes from the list.",
        "Solte uma pasta na janela para abri-la. Enter abre, F2 renomeia, Delete remove da lista.",
    ),
    (
        "No repositories yet. Open, clone or create one, or add a source folder to scan.",
        "Nenhum repositório ainda. Abra, clone ou crie um, ou adicione uma pasta de origem para examinar.",
    ),
    (
        "⚠ {} repositories no longer exist.",
        "⚠ {} repositórios não existem mais.",
    ),
    ("Remove missing", "Remover ausentes"),
    ("⚠ missing", "⚠ ausente"),
    ("(open)", "(aberto)"),
    ("Open", "Abrir"),
    ("Rename (F2)", "Renomear (F2)"),
    ("Move to group", "Mover para o grupo"),
    ("No group", "Sem grupo"),
    ("New group", "Novo grupo"),
    ("Add group", "Adicionar grupo"),
    ("Rename group", "Renomear grupo"),
    ("Delete group", "Excluir grupo"),
    (
        "Empty. Move repositories here from their menu.",
        "Vazio. Mova repositórios para cá pelo menu deles.",
    ),
    ("Add to workspace", "Adicionar ao espaço de trabalho"),
    ("Remove from list (Delete)", "Remover da lista (Delete)"),
    (
        "Found in source folders ({})",
        "Encontrados nas pastas de origem ({})",
    ),
    ("Add all to the list", "Adicionar todos à lista"),
    ("Add to the list", "Adicionar à lista"),
    ("Double-click to open", "Clique duas vezes para abrir"),
    ("Source folders", "Pastas de origem"),
    (
        "Folders where your repositories live. Ferrum lists every repository found in them.",
        "Pastas onde ficam seus repositórios. O Ferrum lista todos os repositórios encontrados nelas.",
    ),
    ("Depth", "Profundidade"),
    (
        "How many folders deep to look",
        "Quantos níveis de pastas examinar",
    ),
    ("+ Add folder…", "+ Adicionar pasta…"),
    ("Scanning…", "Examinando…"),
    ("↻ Rescan", "↻ Examinar de novo"),
    (
        "Look again for new repositories",
        "Procurar novos repositórios outra vez",
    ),
    (
        "Rescan source folders",
        "Examinar as pastas de origem de novo",
    ),
    (
        "{} repositories found in the source folders",
        "{} repositórios encontrados nas pastas de origem",
    ),
    // ----- Workspaces -----
    ("Workspaces", "Espaços de trabalho"),
    (
        "Each workspace remembers its own tabs. Nest them with /.",
        "Cada espaço de trabalho lembra suas próprias abas. Aninhe-os com /.",
    ),
    ("Default workspace", "Espaço de trabalho padrão"),
    ("New workspace", "Novo espaço de trabalho"),
    ("Add", "Adicionar"),
    ("Rename workspace", "Renomear espaço de trabalho"),
    ("Delete workspace", "Excluir espaço de trabalho"),
    ("Workspace: {}", "Espaço de trabalho: {}"),
    ("Move to workspace", "Mover para o espaço de trabalho"),
    (
        "Moved to workspace {}",
        "Movido para o espaço de trabalho {}",
    ),
    // ----- Open Quickly -----
    ("Open Quickly", "Abrir rapidamente"),
    ("Open Quickly…", "Abrir rapidamente…"),
    ("🔍 Open Quickly…", "🔍 Abrir rapidamente…"),
    (
        "Open Quickly…  (Ctrl+Shift+O)",
        "Abrir rapidamente…  (Ctrl+Shift+O)",
    ),
    (
        "Type a repository name…",
        "Digite o nome de um repositório…",
    ),
    ("No matching repository.", "Nenhum repositório corresponde."),
    // ----- Tabs -----
    ("Close Others", "Fechar as outras"),
    ("Close All", "Fechar todas"),
    ("New tab (Ctrl+T)", "Nova aba (Ctrl+T)"),
    ("Reopen closed tab", "Reabrir aba fechada"),
    ("Use the folder name", "Usar o nome da pasta"),
    ("🎨 Color", "🎨 Cor"),
    ("No color", "Sem cor"),
    ("Vertical tab list", "Lista de abas vertical"),
    ("Uncommitted changes", "Alterações não commitadas"),
    ("{} changed files", "{} arquivos alterados"),
    (
        "This view is shown in the main window.",
        "Esta visão aparece na janela principal.",
    ),
    // ----- Opening -----
    ("{} does not exist.", "{} não existe."),
    (
        "{} is not a git repository.",
        "{} não é um repositório git.",
    ),
    (
        "{} is not in a git repository.",
        "{} não está em um repositório git.",
    ),
    ("Trust this folder?", "Confiar nesta pasta?"),
    (
        "Git refuses to open {} because it belongs to another user.",
        "O git se recusa a abrir {} porque ela pertence a outro usuário.",
    ),
    (
        "Adding it to safe.directory in your global git config lets git work in it.",
        "Adicioná-la a safe.directory na sua configuração global do git permite que o git trabalhe nela.",
    ),
    ("Trust and open", "Confiar e abrir"),
    ("Delete the lock file", "Excluir o arquivo de trava"),
    (
        "Git left {} behind, probably after a crash. Delete it only when no other git program is working on this repository.",
        "O git deixou {} para trás, provavelmente após uma falha. Exclua-o só quando nenhum outro programa git estiver trabalhando neste repositório.",
    ),
    // ----- Repository settings -----
    ("Repository settings", "Configurações do repositório"),
    ("Repository settings…", "Configurações do repositório…"),
    (
        "Shared by all worktrees of this repository.",
        "Compartilhadas por todas as worktrees deste repositório.",
    ),
    ("Issue tracker", "Rastreador de issues"),
    (
        "Links from commit messages to issues.",
        "Links das mensagens de commit para issues.",
    ),
    (
        "Edit issue link rules…",
        "Editar regras de links de issues…",
    ),
    ("Pull requests", "Pull requests"),
    ("origin (default)", "origin (padrão)"),
    (
        "Its host is used for pull requests and issue links like #12.",
        "Seu servidor é usado para pull requests e links de issues como #12.",
    ),
    ("Commit template", "Modelo de commit"),
    ("No template", "Sem modelo"),
    (
        "Fills an empty commit message (git config commit.template).",
        "Preenche uma mensagem de commit vazia (git config commit.template).",
    ),
    ("Set commit template", "Definir modelo de commit"),
    // ----- Overview -----
    ("📊 Overview", "📊 Visão geral"),
    (
        "Show repository overview",
        "Mostrar visão geral do repositório",
    ),
    (
        "This repository has no README.",
        "Este repositório não tem README.",
    ),
    ("Counting…", "Contando…"),
    ("Commits", "Commits"),
    ("Contributors", "Colaboradores"),
    ("Local branches", "Branches locais"),
    ("Remote branches", "Branches remotos"),
    ("Size on disk", "Tamanho em disco"),
    ("First commit", "Primeiro commit"),
    ("Last commit", "Último commit"),
    ("Pull requests use {}.", "Pull requests usam {}."),
    ("{} worktrees", "{} worktrees"),
    ("{} submodules", "{} submódulos"),
];
