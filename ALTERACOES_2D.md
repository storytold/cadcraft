# CADCraft — ajustes de compatibilidade 2D (sem alterações 3D)

Esta versão mantém a personalização anterior da Ribbon em português e acrescenta
uma primeira correção no salvamento de desenhos. **Não substitua a instalação do
escritório nem sobrescreva DWGs originais com esta versão experimental.**

## Implementado nesta atualização

1. **Auditoria DWG mais abrangente:** além de entidades desconhecidas, a avaliação
   de perdas agora conta registros com XDATA em tabelas/blocos e entidades que
   possuem reatores persistentes ou nomes de cores de catálogos estrangeiros.
   O aviso de perda ao fazer QSAVE inclui essas categorias quando presentes.
2. **Gravação programática protegida:** `qsave`, `saveas` e `document.bytes`
   aceitam o parâmetro opcional `lossPolicy: "abort"`. Com essa opção, a
   gravação é recusada antes de escrever bytes se uma perda conhecida for
   detectada. O comportamento padrão continua `"allow"` para não quebrar
   automações existentes. O QSAVE pelo menu já possui sua confirmação visual.
3. **Auditoria consistente no Block Editor:** a inspeção do arquivo usa o
   desenho hospedeiro, mesmo enquanto a edição de um bloco está ativa.
4. **Testes de regressão adicionados:** para detecção de metadados e recusa de
   gravação que os perderia.

Exemplo com o canal JSON/CLI, em um desenho aberto:

```json
{"command":"saveas","params":{"path":"copia-protegida.dwg","lossPolicy":"abort"}}
```

Se o arquivo contém dados que o gravador DWG descartaria, a chamada retorna
um erro e não grava o destino. Para DXF, o CADCraft preserva mais objetos
estrangeiros, mas a compatibilidade ainda precisa ser conferida com desenhos
reais de terceiros.

## Pendente — funcionalidades 2D a desenvolver

| Prioridade | Área | Estado |
| --- | --- | --- |
| P0 | Preservação integral de dados DWG, testes com arquivos de terceiros | **Pendente:** a ponte `acadrust` pode perder objetos/metadados. A guarda acima reduz riscos, não resolve a perda. |
| P0 | Importação real de CTB/STB/PC3 e publicação fiel de pranchas | **Pendente:** o CADCraft usa tabelas JSON próprias, não lê os binários da Autodesk. |
| P1 | Geometria de curvas e operações de precisão | **Pendente:** alguns casos com elipses e splines usam aproximações. Polilinhas com bulges já têm parte da implementação. |
| P1 | Autoria de blocos dinâmicos e edição avançada de XREFs | **Pendente:** há suporte parcial a referências existentes. |
| P1 | Interface Ribbon, workspaces, CUIx, paletas | **Parcial:** Ribbon personalizada da atualização anterior; sem importação CUIx. |
| P2 | Cotas, textos, tabelas e hachuras avançadas | **Pendente:** funcionalidades específicas e interoperabilidade incompletas. |
| P2 | Visual LISP completo e APIs de plugins | **Pendente:** mecanismos proprietários não são compatíveis. |

**Modelagem 3D ficou deliberadamente fora do escopo.** Nenhuma funcionalidade
3D foi adicionada, modificada ou prometida nesta atualização.

## Verificação necessária

Neste ambiente de entrega, `cargo`, `rustc` e `rustfmt` não estão instalados;
portanto **não foi possível compilar nem executar os testes Rust novos**.
O `git diff --check` foi executado sem erros de espaçamento.

Em uma máquina com Rust instalado, na pasta do projeto:

```bash
cargo fmt --all -- --check
cargo test -p cadcraft-engine --test save_reports_not_saved
cargo check -p cadcraft
```

Na sequência, para gerar um executável Windows execute `COMPILAR_WINDOWS.bat`.
Não entregue arquivos de produção antes de verificar abertura/salvamento de
cópias e comparar quantitativamente entidades, camadas, atributos e layouts.
