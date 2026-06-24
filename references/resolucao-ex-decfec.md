# Resolução — Exercício de DEC e FEC

Resolução da **Primeira Questão** (sistema de distribuição com três subestações
SD1, SD2 e SD3, operando em 13,8 kV). Enunciado e diagrama em
[`ex-decfec.jpg`](./ex-decfec.jpg). Fórmulas e método em
[`regras-dec-fec.md`](./regras-dec-fec.md).

---

## Dados do enunciado

**Durações das faltas:**

| Falta | Duração | em horas | em minutos |
| ----- | ------- | -------- | ---------- |
| F1    | 160 min | 2,667 h  | 160 min    |
| F2    | 4,60 h  | 4,60 h   | 276 min    |
| F3    | 3,40 h  | 3,40 h   | 204 min    |
| F4    | 2,20 h  | 2,20 h   | 132 min    |
| F5    | 1,60 h  | 1,60 h   | 96 min     |

**Considerações:** (i) nenhuma falta ocorre simultaneamente com outra; (ii) os
números em itálico no diagrama são a quantidade de consumidores em cada bloco.

---

## Ideias-chave da metodologia

> Estes três pontos são exatamente onde a intuição costuma falhar.

1. **DEC/FEC "da chave X" = do alimentador inteiro a jusante de X, somando TODAS
   as faltas que ocorrem nesse alimentador no período** — e não apenas a falta
   citada no item. Por isso:
   
   - chave **1** (SD1) acumula **F1 + F2**;
   - chave **22** (SD2) acumula **F5 + F4**;
   - chave **12** (SD3) acumula **F3**.

2. **Os consumidores sobre o trecho defeituoso só voltam com o REPARO** → o
   tempo deles é a **duração total da falta**. Os demais voltam por
   **transferência** (fechando uma chave NA) nos tempos de manobra do enunciado.

3. **Integração incremental (retangular) da linha do tempo.** Em vez de somar
   `(consumidores do grupo) × (duração total do grupo)`, soma-se, faixa de tempo
   a faixa de tempo, `(quantos ainda estão sem energia) × (duração da faixa)`.
   Os dois caminhos dão o mesmo resultado; o gabarito usa o incremental.

**Fórmulas:**

```
DEC = Σ(Ca · t) / Cc   [h]        FEC = Σ(Ca) / Cc   [interrupções]
```

- No **FEC**, cada falta entra **uma vez** com o total de consumidores que ela
  atingiu (um consumidor atingido por F1 **e** por F2 conta 2 → FEC pode passar de 1).
- No **DEC**, `t` sempre **em horas** (minutos ÷ 60).

---

## Item (a) — DEC e FEC da chave 1 (alimentador SD1)

`Cc = 5400`. Faltas no alimentador: **F1** e **F2**.

### Falta F1 (160 min)

Atinge os **1700** consumidores a jusante do ponto de F1. Não há manobra de
transferência → ficam sem energia pela **duração total**, 160 min.

### Falta F2 (276 min) — restabelecimento escalonado

Atinge **4500** consumidores (a jusante do defeito), restabelecidos em etapas:

| Quando           | Grupo                                   | Consumidores     | Ainda sem energia |
| ---------------- | --------------------------------------- | ---------------- | ----------------- |
| 40 min           | a jusante da chave 6 → SD2              | 1700 restaurados | 2800              |
| 65 min (40+25)   | remanescente a jusante da chave 4 → SD3 | 1700 restaurados | 1100              |
| 276 min (reparo) | trecho defeituoso                       | 1100 restaurados | 0                 |

**Decomposição incremental de F2:** 0–40 min → 4500 fora; 40–65 min (25) → 2800
fora; 65–276 min (211) → 1100 fora.

### Cálculo

```
          (F1)            (F2')           (F2'')          (F2''')
       1700·(160/60)   4500·(40/60)    2800·(25/60)    1100·(211/60)
DEC(1)= ───────────── + ──────────── + ──────────── + ────────────── = 2,33 h
            5400            5400            5400             5400

          (F1)     (F2)
         1700     4500
FEC(1) = ──── + ──── = 6200/5400 = 1,15 interrupções
         5400     5400
```

---

## Item (b) — DEC e FEC da chave 22 (alimentador SD2)

`Cc = 7050`. Faltas no alimentador: **F5** e **F4**.

### Falta F5 (96 min = 1,60 h)

Está junto à cabeceira → derruba **todo** o alimentador: **7050** consumidores.
Sem manobra de transferência → ficam fora pela **duração total**, 1,60 h.

### Falta F4 (132 min) — restabelecimento escalonado

Atinge **3450** consumidores, restabelecidos em etapas:

| Quando           | Grupo                                   | Consumidores     | Ainda sem energia |
| ---------------- | --------------------------------------- | ---------------- | ----------------- |
| 50 min           | a jusante da chave 33 → SD1 (fecha NA3) | 1700 restaurados | 1750              |
| 60 min (50+10)   | a jusante da chave 29 → SD1             | 1350 restaurados | 400               |
| 132 min (reparo) | trecho defeituoso                       | 400 restaurados  | 0                 |

**Decomposição incremental de F4:** 0–50 min → 3450 fora; 50–60 min (10) → 1750
fora; 60–132 min (72) → 400 fora.

### Cálculo

```
          (F5)            (F4')           (F4'')          (F4''')
        7050·(96/60)   3450·(50/60)    1750·(10/60)     400·(72/60)
DEC(22)=───────────── + ──────────── + ──────────── + ───────────── = 2,12 h
            7050            7050            7050             7050

           (F5)     (F4)
          7050     3450
FEC(22) = ──── + ──── = 10500/7050 = 1,49 interrupções
          7050     7050
```

---

## Item (c) — DEC e FEC da chave 12 (alimentador SD3)

`Cc = 5600`. Falta no alimentador: **F3**.

### Falta F3 (204 min) — restabelecimento escalonado

Atinge **4100** consumidores:

| Quando           | Grupo                       | Consumidores     | Ainda sem energia |
| ---------------- | --------------------------- | ---------------- | ----------------- |
| 45 min           | a jusante da chave 17 → SD2 | 2100 restaurados | 2000              |
| 204 min (reparo) | trecho defeituoso           | 2000 restaurados | 0                 |

**Decomposição incremental de F3:** 0–45 min → 4100 fora; 45–204 min (159) → 2000 fora.

### Cálculo

```
          (F3')            (F3'')
       4100·(45/60)    2000·(159/60)
DEC(12)=──────────── + ────────────── = 1,50 h
           5600             5600

          (F3)
         4100
FEC(12) = ──── = 0,73 interrupções
         5600
```

---

## Resumo dos resultados

| Item | Chave | Alimentador | Faltas  | DEC        | FEC      |
| ---- | ----- | ----------- | ------- | ---------- | -------- |
| (a)  | 1     | SD1         | F1 + F2 | **2,33 h** | **1,15** |
| (b)  | 22    | SD2         | F5 + F4 | **2,12 h** | **1,49** |
| (c)  | 12    | SD3         | F3      | **1,50 h** | **0,73** |

### Por que esses três detalhes importam

- **Somar todas as faltas do alimentador:** ignorar F1 (em a) ou F5 (em b)
  subestima fortemente DEC e FEC — e é o que faz o FEC passar de 1,0.
- **Trecho defeituoso espera o reparo:** os blocos de 1100 (a), 400 (b) e 2000 (c)
  ficam sem energia pela duração total da falta (276/132/204 min); são a maior
  parcela do DEC.
- **Tempo em horas:** todos os minutos foram divididos por 60.
