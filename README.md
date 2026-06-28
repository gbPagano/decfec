# DEC/FEC

Calculadora de indicadores DEC/FEC e DIC/FIC/DMIC para redes genéricas de distribuição, com uma biblioteca Rust, uma CLI e uma interface gráfica em `egui`.

Versão web: [decfec.gbpagano.dev](https://decfec.gbpagano.dev)

O projeto modela a rede como um grafo de barramentos e ramos, simula eventos de falta/manobra/reparo ao longo do tempo e calcula os indicadores de continuidade
a partir dos consumidores afetados.

## Recursos

- Cálculo de DEC e FEC por sistema inteiro ou por conjunto a jusante de uma chave.
- Cálculo de DIC, FIC e DMIC para pontos consumidores conectados a ramos de carga.
- Simulação por linha do tempo de eventos, sem informar manualmente quais consumidores foram afetados.
- Entrada e saída em RON para rede, cenário e layout da GUI.
- GUI com edição visual da rede, eventos, labels e layout.
- Persistência automática do rascunho editado no storage da GUI, para não perder trabalho ao atualizar a página.

## CLI

Resumo da rede:

```bash
cargo run -- networks/ref-exercise.ron
```

Consumidores a jusante de uma chave:

```bash
cargo run -- networks/ref-exercise.ron downstream 6
```

DEC/FEC do sistema inteiro:

```bash
cargo run -- networks/ref-exercise.ron dec-fec scenarios/item_a.ron
```

DEC/FEC do conjunto a jusante de uma chave:

```bash
cargo run -- networks/ref-exercise.ron dec-fec scenarios/item_a.ron 2
```

DIC/FIC/DMIC de um ponto consumidor:

```bash
cargo run -- networks/ref-exercise.ron dic-fic-dmic scenarios/item_a.ron X
```

## GUI

Web: [decfec.gbpagano.dev](https://decfec.gbpagano.dev)

Aplicação nativa:

```bash
cargo run -p decfec-gui
```

Aplicação web em desenvolvimento:

```bash
cd gui
trunk serve --open
```

Build web estático:

```bash
cd gui
trunk build --release
```

A GUI carrega uma rede, um cenário e um layout padrão. Alterações feitas pela interface são mantidas no storage da aplicação/navegador.
A rede, o cenário e o layout também podem ser exportados em RON pelos painéis da interface.
No painel de simulação, selecione um ponto consumidor para ver apenas os indicadores individuais DIC, FIC e DMIC.

## Formatos

### Rede

Uma rede contém:

- `buses`: barramentos, subestações, junções e chaves.
- `branches`: ramos entre barramentos, com quantidade de consumidores.

Exemplo reduzido:

```ron
(
    buses: [
        (id: "SD1", kind: Substation),
        (id: "2", kind: Switch(normal: Closed)),
    ],
    branches: [
        (
            id: Some("ramo1"),
            nodes: ["SD1", "2"],
            element: Line(consumers: 0),
        ),
    ],
)
```

### Cenário

Um cenário é uma lista de eventos em minutos:

```ron
(
    events: [
        (at_min: 0.0, bus: "F1", action: Fault),
        (at_min: 160.0, bus: "F1", action: Repair),
    ],
)
```

Ações disponíveis:

- `Fault`: coloca o barramento em falta.
- `Repair`: repara o barramento.
- `Open`: abre uma chave.
- `Close`: fecha uma chave.

### Layout da GUI

O layout salva posições de nós e labels de barramentos ocultos:

```ron
(
    positions: [
        (id: "SD1", x: 59.0, y: -0.6),
    ],
    hidden_bus_labels: [
        "j1",
    ],
)
```

## Validação

A rede passa por validação de mundo fechado:

- ids duplicados são rejeitados.
- ramos precisam referenciar barramentos existentes.
- laços e barramentos desconectados são rejeitados.
- a rede precisa ter pelo menos uma subestação.

Os testes de referência validam checksums do alimentador e o resultado esperado do cenário principal.

```bash
cargo test --test ref-exercise
```
