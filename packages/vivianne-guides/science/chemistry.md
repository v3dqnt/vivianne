# Chemistry

Four tools: inline equations, structure drawings (` ```chem `), 3D molecules (` ```mol3d `), and `check_diagram`. Naming follows IUPAC; write the name in prose and the structure in the figure.

## Inline equations and units: `$\ce{...}$`, `$\pu{...}$`

mhchem is enabled inside `$...$`.

```
$\ce{2H2 + O2 -> 2H2O}$              equation
$\ce{CH3COOH + C2H5OH <=> CH3COOC2H5 + H2O}$   equilibrium
$\ce{SO4^2-}$  $\ce{Fe^3+}$  $\ce{^{14}C}$       charges and isotopes
$\ce{CaCO3 ->[\Delta] CaO + CO2 ^}$   conditions over the arrow, gas evolved
$\pu{9.81 m/s^2}$  $\pu{-286 kJ mol-1}$  $\pu{1.2e-3 mol/L}$   quantities with units
$\Delta H^\circ = -286\ \mathrm{kJ/mol}$, $\alpha$, $\lambda$, $\mu$   other symbols: plain LaTeX
```

Conventions: state symbols `(s) (l) (g) (aq)`; `<=>` for equilibrium, `->` for a reaction that goes; SI units, with kJ/mol and mol/L as the chemistry exceptions.

## Structures: ` ```chem `

One line per structure. A bare line is a SMILES, optionally followed by a name. A line containing `>` is a reaction. A molfile (a block ending in `M  END`) works too.

````
```chem
title: Aspirin
CC(=O)Oc1ccccc1C(=O)O acetylsalicylic acid
highlight: atoms 0-3
```
````

### Options

| Option | Where | Meaning |
|---|---|---|
| `title: text` | whole figure | heading |
| `grid: 3` | whole figure | columns for stand-alone molecules (1 to 8; default chosen from the count) |
| `size: 240` | whole figure | width of one molecule's cell in px (120 to 600) |
| `labels` | whole figure | show atom numbers (RDKit's, counted from 0 in SMILES order) |
| `cip` | whole figure | R/S on stereocentres, E/Z on double bonds |
| `hs` | whole figure | draw hydrogens explicitly |
| `name: text` | a structure | caption under a molecule or reaction |
| `conditions: "above / below" "next arrow"` | a reaction | text on the arrows, in order; the part before ` / ` goes above, after it below; `|` also separates arrows |
| `highlight: atoms 0-3` | a molecule | atoms by number (0-based, as `labels` shows); ranges and commas |
| `highlight: smarts C(=O)O #e34948` | a molecule or reaction | every match of a SMARTS pattern, optional colour |

An option written under a structure belongs to it. An option with no structure above belongs to the one below. `smiles:` may be used in front of a SMILES to be explicit.

Hover a molecule for its formula and molecular weight.

### SMILES you can trust

- Aromatic atoms are lowercase: `c1ccccc1` benzene, `c1ccncc1` pyridine, `[nH]` for pyrrole-type nitrogen.
- Ring closures use digits that appear **twice**: `C1CCCCC1`. An unclosed ring is rejected.
- Stereo: `C[C@H](N)C(=O)O` and `C[C@@H](N)C(=O)O` are the two enantiomers; `F/C=C/F` is trans. Add `cip` to see the labels and check against what you mean.
- Charges and unusual atoms go in brackets: `[NH4+]`, `[O-]`, `[Fe+2]`, `[13CH4]`.
- A salt is disconnected pieces: `[Na+].[Cl-]`.
- Write structures you know. If unsure of a structure, say so and draw the closest you are sure of; do not guess a SMILES for a complicated natural product.
- The drawing is generated, so it will not match a textbook's layout; do not promise it will.

### Reactions and mechanisms

`reactants>agents>products`, with `.` between species. Leave the middle empty (`>>`) and put reagents in `conditions`.

````
```chem
title: Fischer esterification
CC(=O)O.OCC>>CC(=O)OCC.O
conditions: "H2SO4, Δ / reflux"
```
````

A **mechanism** is a chain: put the intermediates one after another, each separated by `>>`. One quoted string per arrow says what happens at that step. Show every charged intermediate with its charge.

````
```chem
title: Fischer esterification, step by step
CC(=O)O>>CC(=[OH+])O>>CC(O)(O)[OH+]CC>>CC([OH2+])(O)OCC>>CC(O)=[O+]CC.O>>CC(=O)OCC
conditions: "H+ / protonation" "EtOH / attack" "proton transfer" "loss of H2O" "− H+"
```
````

Curly arrows are not drawn: describe electron movement in the sentence beside the figure. Formulas in reagent text get subscripts automatically (`H2SO4` → H₂SO₄).

### Several molecules

````
```chem
title: Alcohols
grid: 3
CCO ethanol
CC(C)O propan-2-ol
C(CO)O ethane-1,2-diol
```
````

## 3D molecules and proteins: ` ```mol3d `

3Dmol.js. A PDB entry is fetched from RCSB (needs network); small molecules and other structures can be written inline.

````
```mol3d
pdb: 1CRN
title: Crambin
style: cartoon color=spectrum
style: stick resn=CYS
surface: opacity=0.4 chain=A
label: resi=1 "Thr1"
spin: on
```
````

| Line | Meaning |
|---|---|
| `pdb: 1CRN` | PDB entry (four characters, a digit first) |
| `sdf:` `xyz:` `mol2:` `pdb:` with nothing after the colon | the file's own lines follow, to the end of the block |
| `style: cartoon \| stick \| sphere \| line \| cross` plus options | representation; repeat to combine (cartoon for the chain, sticks for a ligand) |
| selection options | `chain=A`, `resi=10` or `resi=10-25`, `resn=HEM`, `elem=C`, `hetero=true` |
| `color=` | `spectrum`, `chain`, `ss`, `residue`, `element`, `bfactor`, a colour name or `#rrggbb` |
| `radius=` `opacity=` | stick radius or sphere scale; transparency |
| `surface: opacity=0.6 chain=A` | van der Waals surface of a selection |
| `label: resi=10 "Cys10"` | a text on the selected atoms |
| `spin: on` | slow rotation (skipped if the user prefers reduced motion) |
| `background: #ffffff` | default follows the app theme |

Default style is a rainbow cartoon for proteins, sticks for small molecules. Inline example:

````
```mol3d
title: Water
xyz:
3
water
O  0.000  0.000  0.117
H  0.000  0.757 -0.467
H  0.000 -0.757 -0.467
```
````

Use a PDB id only if you are sure of it (1CRN crambin, 1UBQ ubiquitin, 4HHB haemoglobin, 1MBN myoglobin, 2HHB deoxyhaemoglobin, 1HHO oxyhaemoglobin). If the id is wrong the block says RCSB has no such entry.

## Checking

```
check_diagram kind=chem code="CC(=O)O.OCC>>CC(=O)OCC.O"
```

Returns canonical SMILES, formula and molecular weight of every structure drawn, or `INVALID (line N)` with a reason (unclosed ring, unbalanced parenthesis, unknown atom, valence). `C1CC` is rejected: the ring is never closed.
