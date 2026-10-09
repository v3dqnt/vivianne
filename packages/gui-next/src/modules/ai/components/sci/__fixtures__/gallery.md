## Chemistry

Aspirin, with the ester highlighted (atoms 0 to 3):

```chem
title: Aspirin
name: Aspirin
CC(=O)Oc1ccccc1C(=O)O
highlight: atoms 0-3
```

Fischer esterification, the overall reaction and then the mechanism step by step:

```chem
title: Fischer esterification
CC(=O)O.OCC>>CC(=O)OCC.O
conditions: "H2SO4, Δ / reflux"
```

```chem
title: Mechanism
CC(=O)O>>CC(=[OH+])O>>CC(O)(O)[OH+]CC>>CC([OH2+])(O)OCC>>CC(O)=[O+]CC.O>>CC(=O)OCC
conditions: "H+ / protonation" "EtOH / attack" "proton transfer" "− H2O" "− H+"
```

Several molecules, with stereochemistry labelled:

```chem
grid: 3
cip
C[C@H](N)C(=O)O L-alanine
C[C@@H](N)C(=O)O D-alanine
OC[C@H](O)C=O glyceraldehyde
```

Equations: $\ce{2H2 + O2 -> 2H2O}$ with $\Delta H^\circ = -572\ \mathrm{kJ}$; $\ce{CH3COOH + C2H5OH <=> CH3COOC2H5 + H2O}$; $\ce{SO4^2-}$; an acceleration of $\pu{9.81 m/s^2}$; and Greek letters $\alpha$, $\beta$, $\gamma$, $\lambda$, $\omega_0$, $\Omega$.

---

## Biology

```bio
plasmid pUC19
size: 2686
feature: 146..469 "lacZα" cw #3987e5
feature: 1626..2486 "AmpR" ccw #e66767
feature: 867..1455 "ori" cw #1baf7a
site: EcoRI 396
site: HindIII 447
site: BamHI 417
site: SmaI 412
site: PstI 435
site: XbaI 423
```

```bio
sequence
name: GFP start
seq: ATGGTGAGCAAGGGCGAGGAGCTGTTCACCGGGGTGGTGCCCATCCTGGTCGAGCTGGACGGCGACGTAAACGGCCACAAGTTCAGCGTGTCCGGCGAGGGCGAGGGCGATGCC
feature: 1-33 "N-term" #3987e5
feature: 34-90 "β-strand 1" #1baf7a
translate: 1
```

```bio
tree "Ten taxa"
newick: ((((Human:0.006,Chimp:0.007):0.002,Gorilla:0.009):0.009,Orangutan:0.018):0.02,((Macaque:0.04,Baboon:0.04):0.02,((Mouse:0.07,Rat:0.06):0.1,(Dog:0.1,Cat:0.09):0.04):0.05):0.01);
highlight: Human, Chimp, Gorilla "Great apes" #e66767
```

```bio
pathway Glycolysis (excerpt)
compartment cyt "Cytosol"
node glc metabolite "Glucose" in cyt
node g6p metabolite "G6P" in cyt
node f6p metabolite "F6P" in cyt
node fbp metabolite "F1,6BP" in cyt
node HK protein "Hexokinase" in cyt
node GPI protein "GPI" in cyt
node PFK1 protein "PFK-1" in cyt
node cit metabolite "Citrate" in cyt
glc -> g6p by HK
g6p -> f6p by GPI
f6p -> fbp by PFK1
cit inhibits PFK1
g6p inhibits HK
```

```bio
cell animal "Animal cell"
highlight: mitochondrion
```

---

## Plots

```plot
title: Damped oscillator
x: Time t [s] 0..12
y: Displacement x [m] -2..2
param A = 1.5 0..2 step 0.05 "amplitude A [m]"
param g = 0.25 0..1 step 0.01 "damping $\gamma$"
param w = 2.5 0.5..6 step 0.1 "$\omega$ [rad/s]"
fn: A*exp(-g*x)*cos(w*x) "$x(t)$"
fn: A*exp(-g*x) dashed "envelope"
fn: -A*exp(-g*x) dashed
```

```plot
title: Spring constant from data
x: Mass m [kg] 0..5
y: Period squared $T^2$ [s²] 0..8
points: (0.5, 0.9) (1, 1.6) (2, 3.3) (3, 4.7) (4, 6.4) err=0.4 "measured"
fn: 1.6*x "fit"
```

```plot
title: Lissajous curve, a circle and a vector field
x: x -2..2
y: y -2..2
parametric: cos(3*t), sin(2*t) t=0..2pi "Lissajous"
implicit: x^2 + y^2 = 1.5
field: -y, x
```

---

## Physics

```physics
mechanics "Block on a rough incline"
ramp 30deg "θ"
block m on ramp s=4 "m"
force m down 6 "$mg$" #e34948
force m normal 5.2 "$N$" #3987e5
force m up-slope 2.5 "$f$" #1baf7a
```

```physics
circuit "RC circuit"
battery V1 (0,0) (0,4) "9 V"
resistor R1 (0,4) (4,4) "$R$ = 1 kΩ"
capacitor C1 (4,4) (4,0) "$C$ = 100 µF"
wire (4,0) (0,0)
switch S1 (4,4) (8,4) open "S"
wire (8,4) (8,0) (4,0)
ground (0,0)
```

```physics
optics "Converging lens"
lens L at 0 f=8
object at -20 h=3 "object"
```

```physics
wave "Beats"
wave w1 A=1 lambda=4 f=0.5 "w1"
wave w2 A=1 lambda=3 f=0.5 "w2"
sum w1 w2 "w1 + w2"
xrange 0..12
```

```physics
levels "Hydrogen"
hydrogen n=1..5
transition 3 -> 2 "Hα"
transition 2 -> 1 "Lyman α"
```

```physics
field "Electric dipole"
charge q1 +1 at (-2,0)
charge q2 -1 at (2,0)
equipotentials on
```

---

## 3D

```mol3d
pdb: 1CRN
title: Crambin
style: cartoon color=spectrum
```
