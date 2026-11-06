//! Strings of the local changes view, the commit box and their settings.

pub static T: &[(&str, &str)] = &[
    // ----- File lists -----
    ("Stop tracking {}", "Parar de rastrear {}"),
    ("Large files", "Arquivos grandes"),
    (
        "These files are bigger than {} MB and are not tracked by Git LFS:\n\n{}\n\nStage them anyway?",
        "Estes arquivos têm mais de {} MB e não são rastreados pelo Git LFS:\n\n{}\n\nPreparar mesmo assim?",
    ),
    (
        "Staged + last commit ({})",
        "Preparadas + último commit ({})",
    ),
    ("Show files as", "Mostrar arquivos como"),
    ("Flat list", "Lista simples"),
    ("Tree", "Árvore"),
    ("Single list", "Lista única"),
    ("Hide untracked files", "Ocultar arquivos não rastreados"),
    ("Show ignored files", "Mostrar arquivos ignorados"),
    (
        "Add .gitignore template…",
        "Adicionar modelo de .gitignore…",
    ),
    ("List options", "Opções da lista"),
    ("Filter files", "Filtrar arquivos"),
    ("Ignored ({})", "Ignorados ({})"),
    ("Copy relative path", "Copiar caminho relativo"),
    ("Copy full path", "Copiar caminho completo"),
    ("Unstage folder", "Remover pasta da preparação"),
    ("Stage folder", "Preparar pasta"),
    ("Ignore folder", "Ignorar pasta"),
    ("Changes ({})", "Alterações ({})"),
    (
        "{}\nClick the box or press Enter to stage or unstage",
        "{}\nClique na caixa ou pressione Enter para preparar ou remover da preparação",
    ),
    (
        "{}\nDouble-click, press Enter or drag up to unstage",
        "{}\nClique duas vezes, pressione Enter ou arraste para cima para remover da preparação",
    ),
    (
        "{}\nDouble-click, press Enter or drag down to stage",
        "{}\nClique duas vezes, pressione Enter ou arraste para baixo para preparar",
    ),
    ("{} files selected", "{} arquivos selecionados"),
    (
        "Discard the changes in {} files? This cannot be undone.",
        "Descartar as alterações em {} arquivos? Isso não pode ser desfeito.",
    ),
    ("Reset to revision…", "Restaurar para uma revisão…"),
    ("Copy as patch", "Copiar como patch"),
    ("Save as patch…", "Salvar como patch…"),
    ("Ignore", "Ignorar"),
    ("Ignore and stop tracking", "Ignorar e parar de rastrear"),
    ("This file", "Este arquivo"),
    ("These {} files", "Estes {} arquivos"),
    ("Folder {}", "Pasta {}"),
    ("All {} files", "Todos os arquivos {}"),
    ("Custom pattern…", "Padrão personalizado…"),
    // ----- Tools -----
    (
        "There are no changes to put in a patch.",
        "Não há alterações para colocar em um patch.",
    ),
    ("Could not save {}: {}", "Não foi possível salvar {}: {}"),
    ("Ignore by pattern", "Ignorar por padrão"),
    (
        "Pattern to add to .gitignore:",
        "Padrão a adicionar ao .gitignore:",
    ),
    (
        "Examples: *.log, build/, /docs/*.pdf, !keep.log",
        "Exemplos: *.log, build/, /docs/*.pdf, !keep.log",
    ),
    (
        "Stop tracking the selected files ({})",
        "Parar de rastrear os arquivos selecionados ({})",
    ),
    (
        "Removes them from the repository but keeps them on disk (git rm --cached)",
        "Remove-os do repositório, mas os mantém no disco (git rm --cached)",
    ),
    ("Add .gitignore template", "Adicionar modelo de .gitignore"),
    (
        "Patterns of the chosen templates are added to .gitignore (those already there are skipped).",
        "Os padrões dos modelos escolhidos são adicionados ao .gitignore (os que já existem são ignorados).",
    ),
    (
        "Choose one or more templates.",
        "Escolha um ou mais modelos.",
    ),
    (
        "Reset file to a revision",
        "Restaurar arquivo para uma revisão",
    ),
    (
        "Replace {} with its content at:",
        "Substituir {} pelo conteúdo em:",
    ),
    ("Branch, tag or commit", "Branch, tag ou commit"),
    (
        "Uncommitted changes in this file will be lost.",
        "As alterações sem commit neste arquivo serão perdidas.",
    ),
    ("Reset file", "Restaurar arquivo"),
    // ----- Commit box -----
    (
        "⚠ HEAD is detached: the commit will not be on any branch.",
        "⚠ HEAD está desanexado: o commit não ficará em nenhuma branch.",
    ),
    ("Create branch…", "Criar branch…"),
    (
        "⚠ The last commit was already pushed. Amending it rewrites published history and needs a force push.",
        "⚠ O último commit já foi enviado. Corrigi-lo reescreve o histórico publicado e exige um push forçado.",
    ),
    (
        "⚠ Large files not in Git LFS: {}",
        "⚠ Arquivos grandes fora do Git LFS: {}",
    ),
    (
        "Big files make the repository slow to clone. Consider tracking them with Git LFS.",
        "Arquivos grandes deixam o repositório lento para clonar. Considere rastreá-los com o Git LFS.",
    ),
    (
        "Subject length (up to {} recommended, {} at most)",
        "Tamanho do assunto (recomendado até {}, no máximo {})",
    ),
    ("Summary", "Resumo"),
    ("Description (optional)", "Descrição (opcional)"),
    ("Sign off", "Assinar (sign-off)"),
    (
        "Add a Signed-off-by line (--signoff)",
        "Adicionar uma linha Signed-off-by (--signoff)",
    ),
    ("Wrap", "Quebrar linhas"),
    (
        "Wrap the description at column {}",
        "Quebrar as linhas da descrição na coluna {}",
    ),
    ("Recent messages", "Mensagens recentes"),
    ("Stop writing the message", "Parar de escrever a mensagem"),
    ("AI", "IA"),
    (
        "Write the message from the staged changes with {}",
        "Escrever a mensagem a partir das alterações preparadas com {}",
    ),
    (
        "Committed, but there is no branch or remote to push to.",
        "Commit feito, mas não há branch ou remoto para enviar.",
    ),
    ("Ctrl+Enter", "Ctrl+Enter"),
    ("Commit and push", "Fazer commit e push"),
    ("Ctrl+Shift+Enter", "Ctrl+Shift+Enter"),
    ("Push after every commit", "Fazer push após cada commit"),
    ("Running hooks…", "Executando hooks…"),
    ("The commit failed", "O commit falhou"),
    ("Hook output", "Saída dos hooks"),
    ("Commit without hooks", "Fazer commit sem hooks"),
    (
        "Commit again, skipping the pre-commit and commit-msg hooks (--no-verify)",
        "Fazer o commit de novo, pulando os hooks pre-commit e commit-msg (--no-verify)",
    ),
    // ----- AI -----
    (
        "The AI tool stopped unexpectedly.",
        "A ferramenta de IA parou inesperadamente.",
    ),
    ("Nothing is staged.", "Nada está preparado."),
    (
        "Set the AI command in Settings first.",
        "Defina o comando de IA nas Configurações primeiro.",
    ),
    ("Could not start {}: {}", "Não foi possível iniciar {}: {}"),
    ("Cancelled.", "Cancelado."),
    (
        "The AI tool returned an empty message.",
        "A ferramenta de IA retornou uma mensagem vazia.",
    ),
    // ----- Settings -----
    ("✔ Commit", "✔ Commit"),
    ("💡 AI", "💡 IA"),
    (
        "Stage or unstage the selected files",
        "Preparar ou remover da preparação os arquivos selecionados",
    ),
    ("Subject soft limit", "Limite sugerido do assunto"),
    (
        "The length counter turns yellow after this many characters",
        "O contador de tamanho fica amarelo depois desta quantidade de caracteres",
    ),
    ("Subject hard limit", "Limite máximo do assunto"),
    (
        "Characters after this are marked in red",
        "Os caracteres depois deste limite ficam marcados em vermelho",
    ),
    ("Description ruler", "Régua da descrição"),
    (
        "Column of the ruler and of \"Wrap\" (0 hides them)",
        "Coluna da régua e de \"Quebrar linhas\" (0 oculta ambos)",
    ),
    ("Large file warning", "Aviso de arquivo grande"),
    (
        "Warn about files bigger than this that are not in Git LFS (0 turns it off)",
        "Avisar sobre arquivos maiores que isso fora do Git LFS (0 desativa)",
    ),
    (
        "Sign off commits (--signoff)",
        "Assinar commits (--signoff)",
    ),
    (
        "Forget recent messages ({})",
        "Esquecer mensagens recentes ({})",
    ),
    ("AI commit messages", "Mensagens de commit com IA"),
    (
        "The AI button of the commit box runs this command with the prompt on standard input and uses its answer as the message.",
        "O botão IA da caixa de commit executa este comando com o prompt na entrada padrão e usa a resposta como mensagem.",
    ),
    ("Command", "Comando"),
    ("Examples:", "Exemplos:"),
    ("Prompt", "Prompt"),
    (
        "{diff} is replaced by the staged changes",
        "{diff} é substituído pelas alterações preparadas",
    ),
    ("Restore default", "Restaurar padrão"),
];
