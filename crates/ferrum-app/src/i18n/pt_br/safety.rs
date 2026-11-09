//! Strings of the operation timeline, secret detection and learning mode.

pub static T: &[(&str, &str)] = &[
    // ----- Timeline -----
    ("Operation timeline", "Linha do tempo das operações"),
    ("Operation timeline…", "Linha do tempo das operações…"),
    (
        "Operation timeline (restore points)",
        "Linha do tempo das operações (pontos de restauração)",
    ),
    ("Restore to before {}", "Restaurar para antes de {}"),
    ("Restore to before “{}”?", "Restaurar para antes de “{}”?"),
    ("Delete restore point", "Excluir ponto de restauração"),
    (
        "Delete {} restore points",
        "Excluir {} pontos de restauração",
    ),
    (
        "Delete all restore points?",
        "Excluir todos os pontos de restauração?",
    ),
    (
        "Operations done so far can then no longer be undone from the timeline.",
        "As operações feitas até agora não poderão mais ser desfeitas pela linha do tempo.",
    ),
    ("Delete all…", "Excluir tudo…"),
    ("{} restore points", "{} pontos de restauração"),
    ("Restore…", "Restaurar…"),
    ("Restore", "Restaurar"),
    ("Backups", "Backups"),
    ("Show", "Mostrar"),
    ("Today", "Hoje"),
    ("Yesterday", "Ontem"),
    ("on {}", "em {}"),
    ("{} refs changed", "{} refs alteradas"),
    ("{} stashes dropped", "{} stashes descartados"),
    ("1 changed file saved", "1 arquivo alterado salvo"),
    ("{} changed files saved", "{} arquivos alterados salvos"),
    ("{} deleted (was {})", "{} excluída (era {})"),
    ("{} created at {}", "{} criada em {}"),
    ("Failed: {}", "Falhou: {}"),
    ("A {} was in progress", "Havia um {} em andamento"),
    ("stash dropped: {}", "stash descartado: {}"),
    ("Saved files", "Arquivos salvos"),
    (
        "Untracked files were not saved (too large).",
        "Os arquivos não rastreados não foram salvos (grandes demais).",
    ),
    ("No repository", "Nenhum repositório"),
    (
        "Open a repository to see its operations.",
        "Abra um repositório para ver as operações dele.",
    ),
    ("No restore points yet", "Nenhum ponto de restauração ainda"),
    (
        "Ferrum saves one before a rebase, reset, discard, amend, branch delete, stash drop or force push, so any of them can be undone.",
        "O Ferrum salva um antes de rebase, reset, descarte, amend, exclusão de branch, descarte de stash ou push forçado, para que qualquer um deles possa ser desfeito.",
    ),
    (
        "A restore point is saved before every operation that rewrites or drops work. They are kept for {} days.",
        "Um ponto de restauração é salvo antes de cada operação que reescreve ou descarta trabalho. Eles são mantidos por {} dias.",
    ),
    (
        "Restore points are turned off.",
        "Os pontos de restauração estão desligados.",
    ),
    ("HEAD goes back to {}", "O HEAD volta para {}"),
    (
        "Branches and tags are already as they were.",
        "Branches e tags já estão como estavam.",
    ),
    (
        "Branches and tags that move back:",
        "Branches e tags que voltam:",
    ),
    ("{} stashes come back", "{} stashes voltam"),
    ("Branches, tags and files", "Branches, tags e arquivos"),
    (
        "Only branches, tags and stashes (files stay as they are now)",
        "Só branches, tags e stashes (os arquivos ficam como estão agora)",
    ),
    (
        "Uncommitted changes of that time come back: {} files.",
        "As alterações não commitadas daquele momento voltam: {} arquivos.",
    ),
    (
        "The current state, uncommitted changes included, is saved as a restore point first, so this can be undone too.",
        "Antes, o estado atual, com as alterações não commitadas, é salvo como ponto de restauração; então isto também pode ser desfeito.",
    ),
    (
        "The {} in progress is ended.",
        "O {} em andamento é encerrado.",
    ),
    (
        "This restore point is gone: {}",
        "Este ponto de restauração não existe mais: {}",
    ),
    (
        "Could not save a restore point ({}). Turn restore points off in Settings > Safety to run it anyway.",
        "Não foi possível salvar um ponto de restauração ({}). Desligue os pontos de restauração em Configurações > Segurança para executar mesmo assim.",
    ),
    // ----- Secrets -----
    ("Possible secret found", "Possível segredo encontrado"),
    (
        "{} possible secrets found",
        "{} possíveis segredos encontrados",
    ),
    (
        "These lines in the commits to push look like passwords, keys or tokens. Once pushed, others can read them.",
        "Estas linhas nos commits a enviar parecem senhas, chaves ou tokens. Depois do push, outras pessoas podem lê-las.",
    ),
    (
        "These staged lines look like passwords, keys or tokens. Once committed, they stay in the history.",
        "Estas linhas preparadas parecem senhas, chaves ou tokens. Depois do commit, elas ficam no histórico.",
    ),
    (
        "Add “ferrum:allow” to a line to accept it there.",
        "Adicione “ferrum:allow” a uma linha para aceitá-la ali.",
    ),
    ("Add to allowlist", "Adicionar à lista de permitidos"),
    (
        "Accept these values in this repository from now on, then continue",
        "Aceitar estes valores neste repositório daqui em diante e continuar",
    ),
    (
        "Unstage the files with secrets and do not commit",
        "Remover da preparação os arquivos com segredos e não fazer o commit",
    ),
    ("Push anyway", "Fazer push mesmo assim"),
    ("Ignore once", "Ignorar desta vez"),
    ("Nothing was committed.", "Nenhum commit foi feito."),
    ("Nothing was pushed.", "Nenhum push foi feito."),
    ("AWS access key", "Chave de acesso da AWS"),
    ("AWS secret key", "Chave secreta da AWS"),
    ("GitHub token", "Token do GitHub"),
    ("GitHub fine-grained token", "Token refinado do GitHub"),
    ("GitLab token", "Token do GitLab"),
    ("Slack token", "Token do Slack"),
    ("Slack webhook", "Webhook do Slack"),
    ("Stripe key", "Chave do Stripe"),
    ("Google API key", "Chave de API do Google"),
    (
        "Google OAuth client secret",
        "Segredo de cliente OAuth do Google",
    ),
    ("OpenAI API key", "Chave de API da OpenAI"),
    ("Anthropic API key", "Chave de API da Anthropic"),
    ("Private key", "Chave privada"),
    ("JSON Web Token", "JSON Web Token"),
    ("npm token", "Token do npm"),
    ("PyPI token", "Token do PyPI"),
    ("SendGrid key", "Chave do SendGrid"),
    ("Hugging Face token", "Token do Hugging Face"),
    ("DigitalOcean token", "Token da DigitalOcean"),
    ("Azure storage key", "Chave de armazenamento do Azure"),
    ("Password in a URL", "Senha em uma URL"),
    ("Hard-coded secret", "Segredo no código"),
    ("High-entropy string", "Texto de alta entropia"),
    ("Environment file", "Arquivo de ambiente"),
    ("Key file", "Arquivo de chave"),
    ("Credentials file", "Arquivo de credenciais"),
    // ----- Settings -----
    ("🔒 Safety", "🔒 Segurança"),
    ("Safety", "Segurança"),
    ("Restore points", "Pontos de restauração"),
    (
        "Save a restore point before operations that rewrite or drop work",
        "Salvar um ponto de restauração antes de operações que reescrevem ou descartam trabalho",
    ),
    (
        "Rebase, reset, merge, cherry-pick, amend, discard, stash drop, branch delete, force push and history rewrites. Points live in hidden refs (refs/ferrum/restore) and are never pushed.",
        "Rebase, reset, merge, cherry-pick, amend, descarte, descarte de stash, exclusão de branch, push forçado e reescritas de histórico. Os pontos ficam em refs ocultas (refs/ferrum/restore) e nunca são enviados.",
    ),
    ("Keep them for", "Mantê-los por"),
    ("days", "dias"),
    (
        "Include untracked files (up to 100 MB)",
        "Incluir arquivos não rastreados (até 100 MB)",
    ),
    ("Open the timeline", "Abrir a linha do tempo"),
    ("Secret detection", "Detecção de segredos"),
    (
        "Scan staged changes for secrets before committing",
        "Procurar segredos nas alterações preparadas antes do commit",
    ),
    (
        "Scan the commits before pushing",
        "Verificar os commits antes do push",
    ),
    (
        "Look for random-looking strings (high entropy)",
        "Procurar textos com cara de aleatórios (alta entropia)",
    ),
    (
        "Warn about .env, key and credential files",
        "Avisar sobre arquivos .env, de chaves e de credenciais",
    ),
    ("Built-in rules", "Regras incluídas"),
    ("Custom patterns", "Padrões personalizados"),
    (
        "Regular expressions; the first group (or the whole match) is the secret.",
        "Expressões regulares; o primeiro grupo (ou a correspondência inteira) é o segredo.",
    ),
    ("Add pattern", "Adicionar padrão"),
    ("Allowlists", "Listas de permitidos"),
    (
        "Values accepted with “Add to allowlist”, and path patterns (one per line, e.g. tests/fixtures/** or *.pem) skipped, per repository.",
        "Valores aceitos com “Adicionar à lista de permitidos” e padrões de caminho ignorados (um por linha, por exemplo tests/fixtures/** ou *.pem), por repositório.",
    ),
    ("Nothing allowed yet.", "Nada permitido ainda."),
    ("{} values", "{} valores"),
    ("Paths to skip", "Caminhos a ignorar"),
    ("Learning mode", "Modo de aprendizado"),
    (
        "Show the git commands of each action, with a short explanation",
        "Mostrar os comandos git de cada ação, com uma breve explicação",
    ),
    (
        "A panel lists the commands as they run, and button tooltips show the command they use.",
        "Um painel lista os comandos enquanto rodam, e as dicas dos botões mostram o comando que eles usam.",
    ),
    // ----- Learning mode -----
    (
        "The git commands behind each action, as they run.",
        "Os comandos git por trás de cada ação, enquanto rodam.",
    ),
    ("Nothing run yet", "Nada executado ainda"),
    (
        "Fetch, commit, check out a branch… the git commands it uses show up here.",
        "Fetch, commit, checkout de uma branch… os comandos git usados aparecem aqui.",
    ),
    (
        "Downloads new commits, branches and tags from the remote. Your branches and files do not change.",
        "Baixa novos commits, branches e tags do remoto. Suas branches e arquivos não mudam.",
    ),
    (
        "Fetches the upstream branch and replays your local commits on top of it.",
        "Busca a branch upstream e reaplica seus commits locais sobre ela.",
    ),
    (
        "Fetches the upstream branch and merges it into the current branch.",
        "Busca a branch upstream e faz merge dela na branch atual.",
    ),
    (
        "Deletes a branch or tag on the remote.",
        "Exclui uma branch ou tag no remoto.",
    ),
    (
        "Uploads your commits and replaces the remote branch, unless someone else pushed to it meanwhile.",
        "Envia seus commits e substitui a branch remota, a menos que outra pessoa tenha feito push nela nesse meio-tempo.",
    ),
    (
        "Uploads your commits to the remote branch.",
        "Envia seus commits para a branch remota.",
    ),
    (
        "Replaces the last commit with a new one that also has the staged changes.",
        "Substitui o último commit por um novo que também tem as alterações preparadas.",
    ),
    (
        "Records the staged changes as a new commit.",
        "Grava as alterações preparadas como um novo commit.",
    ),
    (
        "Stages changes: they go into the next commit.",
        "Prepara alterações: elas entram no próximo commit.",
    ),
    (
        "Moves the current branch to another commit and throws away all local changes.",
        "Move a branch atual para outro commit e descarta todas as alterações locais.",
    ),
    (
        "Moves the current branch to another commit; the changes stay staged.",
        "Move a branch atual para outro commit; as alterações continuam preparadas.",
    ),
    (
        "Moves the current branch to another commit, keeping local changes that do not conflict.",
        "Move a branch atual para outro commit, mantendo as alterações locais que não conflitam.",
    ),
    (
        "Unstages changes: they stay in the files but leave the next commit.",
        "Remove alterações da preparação: elas continuam nos arquivos, mas saem do próximo commit.",
    ),
    (
        "Moves the current branch to another commit; the changes stay in the files.",
        "Move a branch atual para outro commit; as alterações continuam nos arquivos.",
    ),
    (
        "Creates a branch and switches to it.",
        "Cria uma branch e muda para ela.",
    ),
    (
        "Replaces files with their last committed version.",
        "Substitui arquivos pela última versão commitada.",
    ),
    (
        "Switches to another branch or commit, updating the files.",
        "Muda para outra branch ou commit, atualizando os arquivos.",
    ),
    (
        "Deletes a branch (the commits stay reachable from the reflog for a while).",
        "Exclui uma branch (os commits continuam acessíveis pelo reflog por um tempo).",
    ),
    ("Renames a branch.", "Renomeia uma branch."),
    (
        "Moves a branch to another commit.",
        "Move uma branch para outro commit.",
    ),
    (
        "Creates a branch without switching to it.",
        "Cria uma branch sem mudar para ela.",
    ),
    (
        "Stops the merge and puts everything back as it was before it.",
        "Interrompe o merge e deixa tudo como estava antes dele.",
    ),
    (
        "Joins another branch's history into the current branch.",
        "Junta o histórico de outra branch à branch atual.",
    ),
    (
        "Stops the rebase and puts the branch back as it was before it.",
        "Interrompe o rebase e deixa a branch como estava antes dele.",
    ),
    (
        "Goes on with the rebase after conflicts are resolved.",
        "Continua o rebase depois que os conflitos são resolvidos.",
    ),
    (
        "Replays the current branch's commits on top of another commit, rewriting them.",
        "Reaplica os commits da branch atual sobre outro commit, reescrevendo-os.",
    ),
    (
        "Copies a commit onto the current branch.",
        "Copia um commit para a branch atual.",
    ),
    (
        "Makes a new commit that undoes the changes of an earlier one.",
        "Cria um novo commit que desfaz as alterações de um anterior.",
    ),
    (
        "Applies the latest stash to the files and deletes it.",
        "Aplica o stash mais recente aos arquivos e o exclui.",
    ),
    (
        "Applies a stash to the files and keeps it.",
        "Aplica um stash aos arquivos e o mantém.",
    ),
    ("Deletes a stash.", "Exclui um stash."),
    (
        "Puts a commit back in the stash list.",
        "Coloca um commit de volta na lista de stashes.",
    ),
    (
        "Puts local changes aside in a stash and cleans the working tree.",
        "Guarda as alterações locais em um stash e limpa a cópia de trabalho.",
    ),
    ("Deletes a tag.", "Exclui uma tag."),
    (
        "Gives a commit a permanent name, such as a version number.",
        "Dá a um commit um nome permanente, como um número de versão.",
    ),
    (
        "Deletes untracked files.",
        "Exclui arquivos não rastreados.",
    ),
    (
        "Applies a patch to the files or the index (staging part of a file).",
        "Aplica um patch aos arquivos ou ao índice (preparando parte de um arquivo).",
    ),
    (
        "Removes files from git (with --cached: keeps them on disk).",
        "Remove arquivos do git (com --cached: mantém no disco).",
    ),
    (
        "Manages the remotes: the other repositories you fetch from and push to.",
        "Gerencia os remotos: os outros repositórios de onde você faz fetch e para onde faz push.",
    ),
    (
        "Lists the branches and tags of a remote, to test the connection.",
        "Lista as branches e tags de um remoto, para testar a conexão.",
    ),
    (
        "Low-level: points a ref (branch, tag, restore point) at a commit.",
        "Baixo nível: aponta uma ref (branch, tag, ponto de restauração) para um commit.",
    ),
    (
        "Low-level: sets the branch HEAD points to.",
        "Baixo nível: define a branch para a qual o HEAD aponta.",
    ),
    (
        "Low-level: loads a saved tree into the index (and the files, with -u).",
        "Baixo nível: carrega uma árvore salva no índice (e nos arquivos, com -u).",
    ),
    (
        "Low-level: saves the index as a tree object.",
        "Baixo nível: salva o índice como um objeto árvore.",
    ),
    (
        "Low-level: creates a commit object from a tree.",
        "Baixo nível: cria um objeto commit a partir de uma árvore.",
    ),
    (
        "Low-level: stores data in the object database.",
        "Baixo nível: guarda dados no banco de objetos.",
    ),
    (
        "Looks up a commit id or a path inside the repository.",
        "Procura o id de um commit ou um caminho dentro do repositório.",
    ),
    (
        "Updates the repositories nested in this one.",
        "Atualiza os repositórios aninhados neste.",
    ),
    (
        "Git LFS: large files kept outside the repository.",
        "Git LFS: arquivos grandes guardados fora do repositório.",
    ),
    (
        "Manages extra working folders of the same repository.",
        "Gerencia pastas de trabalho extras do mesmo repositório.",
    ),
    (
        "Reads or changes a git setting.",
        "Lê ou altera uma configuração do git.",
    ),
    (
        "Finds the commit that introduced a bug by testing commits in between.",
        "Encontra o commit que introduziu um bug testando os commits intermediários.",
    ),
    (
        "Applies patches from mail files as commits.",
        "Aplica patches de arquivos de e-mail como commits.",
    ),
    (
        "Writes commits as patch files.",
        "Grava commits como arquivos de patch.",
    ),
    (
        "Restores files from the index or a commit.",
        "Restaura arquivos do índice ou de um commit.",
    ),
    (
        "Shows which files changed.",
        "Mostra quais arquivos mudaram.",
    ),
    (
        "Shows the changes between two versions.",
        "Mostra as alterações entre duas versões.",
    ),
    ("Lists commits.", "Lista commits."),
    (
        "Shows a commit or a file at a commit.",
        "Mostra um commit ou um arquivo em um commit.",
    ),
    (
        "Cleans up and compresses the repository.",
        "Limpa e compacta o repositório.",
    ),
];
