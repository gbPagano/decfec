# Regras de Cálculo de DEC e FEC

Indicadores **coletivos** de continuidade de serviço usados em sistemas de
distribuição de energia (referência: ANEEL — PRODIST, Módulo 8). Medem a
qualidade do fornecimento de um **conjunto** de unidades consumidoras (um
alimentador, uma subestação, um município, uma área de concessão, etc.).

- **DEC** — *Duração Equivalente de Interrupção por Unidade Consumidora*.
  Quanto tempo, em média, cada consumidor do conjunto ficou sem energia no período.
- **FEC** — *Frequência Equivalente de Interrupção por Unidade Consumidora*.
  Quantas vezes, em média, cada consumidor do conjunto foi interrompido no período.

---

## 1. Fórmulas

Para um conjunto com `Cc` unidades consumidoras, sujeito a `k` interrupções no
período de apuração:

```
        Σ ( Ca(i) · t(i) )          i = 1 .. k
DEC  =  ─────────────────────
               Cc

        Σ ( Ca(i) )                 i = 1 .. k
FEC  =  ─────────────────────
               Cc
```

Onde:

| Símbolo | Significado                                                             | Unidade      |
| ------- | ----------------------------------------------------------------------- | ------------ |
| `Ca(i)` | número de consumidores **atingidos** (interrompidos) na interrupção `i` | consumidores |
| `t(i)`  | **duração** da interrupção `i`                                          | horas        |
| `Cc`    | número **total** de consumidores do conjunto considerado                | consumidores |
| `k`     | número de interrupções ocorridas no período                             | —            |

- **DEC** resulta em **horas** (h por consumidor, no período).
- **FEC** é **adimensional** (número de interrupções por consumidor, no período).

> Atenção às unidades: `t(i)` entra **em horas**. Tempos dados em minutos devem
> ser divididos por 60 (ex.: `40 min = 40/60 = 0,667 h`).

---

## 2. Relação com os indicadores individuais (DIC / FIC)

Os indicadores **individuais** descrevem **uma** unidade consumidora `j`:

```
DIC(j)  = Σ t(i)         (soma das durações das interrupções que atingem j)
FIC(j)  = número de interrupções que atingem j
DMIC(j) = max t(i)       (maior duração individual de interrupção de j)
```

Os indicadores coletivos são a **média** dos individuais sobre o conjunto:

```
DEC = ( Σ DIC(j) ) / Cc          FEC = ( Σ FIC(j) ) / Cc
       j = 1 .. Cc                       j = 1 .. Cc
```

Ou seja: somar `Ca(i)·t(i)` por evento é equivalente a somar `DIC(j)` por
consumidor — os dois caminhos dão o mesmo DEC.

---

## 3. Procedimento prático de cálculo

1. **Definir o conjunto** e contar `Cc` (total de consumidores).
2. **Listar TODAS as interrupções do período** que atingem esse conjunto — não
   apenas uma. O DEC/FEC de um alimentador soma **todas as faltas que ocorrem
   nele** (ex.: se F1 e F2 atingem o mesmo alimentador, **ambas** entram na conta).
3. Para cada falta, identificar:
   - quais consumidores ficaram sem energia → `Ca(i)`;
   - por quanto tempo cada grupo ficou sem energia → `t(i)` (em horas).
4. Quando uma mesma falta restabelece **grupos diferentes em tempos diferentes**
   (manobras escalonadas + reparo), tratar **cada grupo como uma parcela** com
   seu próprio `Ca` e seu próprio `t` (ver §3.2, método incremental).
5. **Somar** `Ca·t` (DEC) e `Ca` (FEC), depois **dividir** por `Cc`.

### 3.1 Quem entra como "atingido", e por quanto tempo

- Consumidores **a jusante do defeito** ficam sem energia e voltam por **manobra
  de transferência** para outra fonte (fechando uma chave NA — *Normalmente
  Aberta*). O `t` deles é o **tempo de manobra** até essa transferência.
- Consumidores **a montante do defeito** (entre a fonte e o defeito) são
  re-energizados pela **própria subestação** ao se isolar o trecho defeituoso.
  Têm duração desprezível e **não compõem** o numerador (mas seguem em `Cc`).
- Consumidores **no próprio trecho defeituoso** **só voltam com o REPARO** → seu
  `t` é a **duração total da falta**. Costuma ser a maior parcela do DEC, então
  **não esqueça desse grupo**.

### 3.2 Método incremental (integração retangular da linha do tempo)

Para uma falta que restabelece a carga em etapas, em vez de calcular grupo a
grupo `(consumidores) × (duração total do grupo)`, percorre-se a **linha do
tempo** somando, em cada faixa, `(quantos ainda estão sem energia) × (duração da
faixa)`:

```
falta com restabelecimentos em t1 < t2 < ... < t_reparo:

DEC_falta · Cc =  N0·(t1−0) + N1·(t2−t1) + ... + N_último·(t_reparo − t_anterior)

   N0 = total atingido (0→t1);  N1 = ainda fora após a 1ª manobra;  ...
```

Exemplo (item a, falta F2; tempos em min): atinge 4500; em 40 min volta 1700;
em 65 min volta mais 1700; em 276 min (reparo) voltam os 1100 do trecho:

```
4500·(40) + 2800·(25) + 1100·(211)      [211 = 276 − 65]
```

Resultado idêntico ao de somar `1700·40 + 1700·65 + 1100·276`. Use o que preferir.

---

## 4. Conceitos auxiliares da rede

- **Chave NA (Normalmente Aberta)** — interligação entre alimentadores/
  subestações que fica **aberta** na operação normal (rede radial). Ao **fechá-la**
  (e abrir uma chave seccionadora a montante), transfere-se carga para outra
  fonte. É o mecanismo de "transferência" citado no enunciado.
- **Chave NF (Normalmente Fechada) / seccionadora** — fica fechada na operação
  normal; é **aberta** para isolar o trecho defeituoso.
- **"A jusante da chave X"** — todos os consumidores alcançados a partir da
  chave X **no sentido contrário à fonte** (afastando-se da subestação).
- **DEC/FEC "da chave X"** — indicadores **do conjunto de consumidores a jusante
  da chave X** (no caso de uma chave de cabeceira, é o alimentador inteiro).

---

## 5. Resumo de bolso

```
DEC = Σ(Ca·t) / Cc     [h]       →  "horas médias sem energia por consumidor"
FEC = Σ(Ca)   / Cc     [-]       →  "interrupções médias por consumidor"
```

- Tempo **sempre em horas**.
- Some **todas as faltas** do alimentador no período (não só uma). Um consumidor
  atingido por duas faltas conta **duas vezes** no FEC → o **FEC pode passar de 1,0**.
- Cada grupo restabelecido em tempo diferente = **uma parcela** no somatório do DEC.
- **Não esqueça o trecho defeituoso:** esses consumidores ficam fora pela
  **duração total da falta** (só voltam no reparo).
- `Cc` = **todos** os consumidores do conjunto (mesmo os não atingidos).
