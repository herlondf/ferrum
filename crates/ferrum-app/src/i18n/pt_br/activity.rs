//! Strings of the activity window, status control and background settings.

pub static T: &[(&str, &str)] = &[
    // ----- Status control and activity window -----
    ("Activity", "Atividade"),
    (
        "Activity: operations and their git commands",
        "Atividade: operações e seus comandos git",
    ),
    ("All", "Todas"),
    ("User", "Usuário"),
    ("Background", "Em segundo plano"),
    ("(background)", "(em segundo plano)"),
    ("All repositories", "Todos os repositórios"),
    ("Clear finished", "Limpar concluídas"),
    ("Copy output", "Copiar saída"),
    ("No operations yet.", "Nenhuma operação ainda."),
    (
        "No git commands were run.",
        "Nenhum comando git foi executado.",
    ),
    ("Cancel {}", "Cancelar {}"),
    ("✖ Cancel", "✖ Cancelar"),
    ("{} cancelled", "{} cancelado"),
    ("Cancelled", "Cancelada"),
    ("Done", "Concluída"),
    ("Running…", "Executando…"),
    ("running…", "executando…"),
    ("running since {}", "executando desde {}"),
    ("succeeded {}", "concluída {}"),
    ("failed {}", "falhou {}"),
    ("cancelled {}", "cancelada {}"),
    ("cancelled", "cancelado"),
    ("did not start", "não iniciou"),
    ("exit code {} · {}", "código de saída {} · {}"),
    // ----- Settings -----
    ("Refresh and notifications", "Atualização e notificações"),
    (
        "Watch the working folder for changes",
        "Observar alterações na pasta de trabalho",
    ),
    (
        "Reload as soon as files, branches or the index change on disk",
        "Recarregar assim que arquivos, branches ou o índice mudarem no disco",
    ),
    ("Otherwise check every", "Caso contrário, verificar a cada"),
    (
        "Refresh when the window gets the focus",
        "Atualizar quando a janela receber o foco",
    ),
    (
        "Notify when a long operation finishes while the window is in the background",
        "Notificar quando uma operação longa terminar com a janela em segundo plano",
    ),
    (
        "Check for new versions once a day",
        "Procurar novas versões uma vez por dia",
    ),
    ("Version {} available", "Versão {} disponível"),
];
