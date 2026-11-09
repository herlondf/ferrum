//! Strings of the structural diff, the semantic merge and the conflict editor.

pub static T: &[(&str, &str)] = &[
    // ----- Structural diff -----
    ("Structural diff", "Diff estrutural"),
    (
        "Compare the code's syntax: formatting changes are ignored and moved code is marked. Remembered per repository.",
        "Compara a sintaxe do código: mudanças só de formatação são ignoradas e código movido é marcado. Lembrado por repositório.",
    ),
    ("Structural (syntax-aware)", "Estrutural (pela sintaxe)"),
    ("Comparing the syntax…", "Comparando a sintaxe…"),
    ("No syntax changes", "Nenhuma mudança de sintaxe"),
    (
        "Both versions have the same content.",
        "As duas versões têm o mesmo conteúdo.",
    ),
    (
        "Only formatting changed: whitespace, indentation or line breaks.",
        "Só a formatação mudou: espaços, indentação ou quebras de linha.",
    ),
    (
        "Structural diff unavailable ({}): showing the line diff.",
        "Diff estrutural indisponível ({}): mostrando o diff por linhas.",
    ),
    (
        "no grammar for this file",
        "não há gramática para este arquivo",
    ),
    ("the file is too large", "o arquivo é grande demais"),
    ("the file is not UTF-8 text", "o arquivo não é texto UTF-8"),
    ("the file is too complex", "o arquivo é complexo demais"),
    ("Structural · {}", "Estrutural · {}"),
    ("{} removed", "{} removidos"),
    ("{} added", "{} adicionados"),
    ("{} moved", "{} movidos"),
    (
        "syntax errors: the result may be rough",
        "erros de sintaxe: o resultado pode ser impreciso",
    ),
    ("{} unchanged lines", "{} linhas inalteradas"),
    // ----- Semantic merge -----
    (
        "Resolved automatically — review",
        "Resolvido automaticamente — revise",
    ),
    ("by mergiraf", "pelo mergiraf"),
    (
        "by Ferrum's semantic merge",
        "pelo merge semântico do Ferrum",
    ),
    ("Dismiss the suggestion", "Descartar a sugestão"),
    ("In the result below", "No resultado abaixo"),
    ("Use this result", "Usar este resultado"),
    (
        "Put the merged file in the result editor; nothing is staged until you mark it as resolved",
        "Coloca o arquivo mesclado no editor de resultado; nada é preparado até você marcá-lo como resolvido",
    ),
    (
        "Both sides' changes were merged by syntax. Check the result before marking the file as resolved.",
        "As mudanças dos dois lados foram mescladas pela sintaxe. Confira o resultado antes de marcar o arquivo como resolvido.",
    ),
    ("… and {} more", "… e mais {}"),
    ("Compare with", "Comparar com"),
    ("added on {}", "adicionado em {}"),
    ("changed on {}", "alterado em {}"),
    ("deleted on {}", "excluído em {}"),
    ("same change on both sides", "mesma mudança nos dois lados"),
    (
        "changed on both sides, merged line by line",
        "alterado nos dois lados, mesclado linha a linha",
    ),
    (
        "a version has syntax errors",
        "uma versão tem erros de sintaxe",
    ),
    (
        "a version contains conflict markers",
        "uma versão contém marcadores de conflito",
    ),
    ("both sides changed {}", "os dois lados alteraram {}"),
    (
        "both sides added code at the same place",
        "os dois lados adicionaram código no mesmo lugar",
    ),
    (
        "both sides added code at the same place in {}",
        "os dois lados adicionaram código no mesmo lugar em {}",
    ),
    (
        "The semantic merge left this file to you: {}.",
        "O merge semântico deixou este arquivo para você: {}.",
    ),
    (
        "1 file can be resolved automatically",
        "1 arquivo pode ser resolvido automaticamente",
    ),
    (
        "{} files can be resolved automatically",
        "{} arquivos podem ser resolvidos automaticamente",
    ),
    ("Review the suggested merge", "Revisar o merge sugerido"),
    (
        "Structural diff and semantic merge",
        "Diff estrutural e merge semântico",
    ),
    (
        "Offer the structural diff (Structural in the diff toolbar)",
        "Oferecer o diff estrutural (Estrutural na barra do diff)",
    ),
    (
        "Try a semantic merge of conflicted files and suggest the result",
        "Tentar um merge semântico dos arquivos em conflito e sugerir o resultado",
    ),
    (
        "Also try mergiraf when it is installed",
        "Tentar também o mergiraf quando estiver instalado",
    ),
    ("found: {}", "encontrado: {}"),
    ("not found on the PATH", "não encontrado no PATH"),
    ("Languages: {}.", "Linguagens: {}."),
    // ----- Conflict editor -----
    (
        "Every conflicted file is resolved: continue the operation from the banner.",
        "Todos os arquivos em conflito foram resolvidos: continue a operação pela faixa acima.",
    ),
    (
        "{} of {} conflicts resolved",
        "{} de {} conflitos resolvidos",
    ),
    (
        "Previous unresolved conflict",
        "Conflito não resolvido anterior",
    ),
    ("Next unresolved conflict", "Próximo conflito não resolvido"),
    ("All {}", "Tudo de {}"),
    (
        "Take {} in every unresolved conflict",
        "Usar {} em todos os conflitos não resolvidos",
    ),
    ("Show base", "Mostrar base"),
    (
        "Show the common ancestor between both sides",
        "Mostra o ancestral comum entre os dois lados",
    ),
    (
        "The semantic merge result: check it, then mark the file as resolved.",
        "O resultado do merge semântico: confira e depois marque o arquivo como resolvido.",
    ),
];
