# Physics and mathematics

Two fences. ` ```plot ` draws graphs of functions and data, with sliders for parameters. ` ```physics ` draws diagrams (free-body, optics, fields, circuits, waves, energy levels) as interactive constructions. For calculation use the `math` tool; draw what it gives you.

Conventions: **SI units**, written in the label (`[m/s]`); vectors drawn as arrows with the symbol beside the tip; a free-body diagram shows the forces *on* the body only, tail at the body; circuit symbols are IEC/IEEE standard; every axis has a name and a unit.

## `plot`

````
```plot
title: Damped oscillator
x: Time t [s] 0..12
y: Displacement x [m] -2..2
param A = 1.5 0..2 step 0.05 "amplitude A [m]"
param g = 0.25 0..1 step 0.01 "damping $\gamma$"
param w = 2.5 0.5..6 step 0.1 "$\omega$ [rad/s]"
fn: A*exp(-g*x)*cos(w*x) "$x(t)$"
fn: A*exp(-g*x) dashed "envelope"
```
````

| Line | Meaning |
|---|---|
| `title:` | heading |
| `x: Label [unit] a..b [log]` | x axis: label, unit, range, optional log scale. Ranges may use `pi` (`0..2pi`). `y:` likewise |
| `grid: off` | no grid |
| `height: 340` | plot height in px |
| `param A = 1.5 0..3 step 0.1 "label"` | a slider; `A` can be used in any expression. No colon after `param` |
| `fn: expression "label" #colour dashed\|dotted\|thick` | y as a function of `x` |
| `polar: 1 + cos(theta) theta=0..2pi` | r as a function of `theta` (range optional) |
| `parametric: cos(3*t), sin(2*t) t=0..2pi` | x(t), y(t) separated by a comma |
| `implicit: x^2 + y^2 = 4` | the curve where the two sides are equal |
| `field: -y, x [density=14]` | a vector field (arrows) from its two components in `x`, `y` |
| `points: (1, 2) (2, 4, 0.3) err=0.2 "data" [line]` | data; `(x, y)`, `(x, y, yerr)` or `(x, y, xerr, yerr)`; `err=` sets a default y error; `line` joins the points |
| `hline: 0.5 "label"` / `vline: 3` | reference lines |

Expressions: numbers, `x`, parameter names, `+ - * / ^`, parentheses, and `sin cos tan asin acos atan atan2 sinh cosh tanh exp log ln log10 log2 sqrt cbrt abs sign floor ceil round min max pow mod fact step sinc`; constants `pi e tau`. Write `2*x`, not `2x`. The reader can zoom (Ctrl+scroll) and pan.

Axis labels are plain text: write Greek as characters (`ω`) or `\omega`, not `$...$`. Series and slider labels may use `$...$`.

Data with error bars and a log axis:

````
```plot
title: Bode plot of an RC low-pass filter
x: Frequency f [Hz] 1..100000 log
y: Gain |H| 0.001..2 log
param RC = 0.001 0.0001..0.01 step 0.0001 "$RC$ [s]"
fn: 1/sqrt(1 + (2*pi*x*RC)^2) "$|H(f)|$"
vline: 159 "cut-off"
```
````

## `physics`

The first line is the kind and an optional title: `physics mechanics "Title"` (the word `physics` is optional inside the fence: `mechanics "Title"` works). Kinds: `mechanics` (also `free-body`, `incline`, `spring`, `pulley`), `optics`, `field`, `circuit`, `wave`, `levels`. One statement per line; labels in `"quotes"` may use `$...$`; points are `(x, y)` in drawing units, y up; colours `#rrggbb`.

### mechanics

````
```physics
mechanics "Block on a rough incline"
ramp 30deg "θ"
block m on ramp s=4 "m"
force m down 6 "$mg$" #e34948
force m normal 5.2 "$N$" #3987e5
force m up-slope 2.5 "$f$" #1baf7a
```
````

| Statement | Meaning |
|---|---|
| `ramp 30deg [length=8] ["θ"]` | an incline rising from the origin to the right, ground hatched, angle arc |
| `floor [y=0] [x1=-6 x2=6]`, `wall [x=0] [y1=0 y2=4]` | hatched surfaces |
| `block id at (x,y) [size=2x1.2] ["label"]` | a rectangle; `block id on ramp s=4` puts it on the incline `s` along the surface |
| `ball id at (x,y) [r=0.5] ["label"]` | a circle |
| `pulley id at (x,y) [r=0.6]` | a wheel with a support |
| `spring A B [coils=8] ["k"]` | a coil between two objects or points; ends are clipped to the outline |
| `rope A B ...` | a line through objects and points; through a `pulley` it wraps over the top |
| `force target direction size ["label"] [#colour] [dashed] [scale=0.55]` | an arrow from the target's centre; length = size × scale |
| `arrow (x,y) (x,y) ["label"]`, `line (x,y) (x,y) [dashed]`, `dim (x,y) (x,y) "4 m"`, `label (x,y) "text"`, `axes` | annotations |

Directions: `up down left right`, an angle (`30deg`, from the +x axis), `weight`, and with a ramp `normal`, `up-slope`, `down-slope`, `into-slope`. The size is in newtons or any consistent unit.

Free-body diagram of a block on a table and an Atwood machine:

````
```physics
free-body "Forces on a block"
block b at (0,0) "m"
force b down 4 "$mg$" #e34948
force b up 4 "$N$" #3987e5
force b right 2.5 "$F$" #1baf7a
force b left 1.2 "$f$" #e5832b
floor y=-0.6
```
````

````
```physics
pulley "Atwood machine"
pulley P at (0,6) r=0.6
block a at (-0.6,3) size=1.2x1.2 "m1"
block b at (0.6,2) size=1.2x1.2 "m2"
rope a P b
force a down 3 "$m_1 g$" #e34948
force b down 4 "$m_2 g$" #e34948
```
````

### optics

Distances in cm, axis horizontal, light from the left. The first lens or mirror forms an image of the object, with the three principal rays.

````
```physics
optics "Converging lens, object beyond 2f"
lens L at 0 f=8
object at -20 h=3 "object"
```
````

| Statement | Meaning |
|---|---|
| `lens id at x f=8 [h=6]` | thin lens; negative `f` is diverging; foci F and F' marked |
| `mirror id at x concave\|convex\|plane f=8 [h=6]` | reflecting surface faces left |
| `object at x [h=3] ["label"]` | an upright arrow on the axis |
| `rays off` | draw only the elements and image, no rays |
| `interface id at x n1=1 n2=1.5` | a vertical boundary between two media |
| `ray (x,y) angle=30 [#colour]` | a ray refracted at every interface by Snell's law (total internal reflection is reported) |
| `screen at x`, `axis a..b`, `note "text"` | extras |

The image position and magnification are printed below the figure (1/f = 1/v − 1/u); quote them in your explanation after checking.

### field

````
```physics
field "Electric dipole"
charge q1 +1 at (-2,0)
charge q2 -1 at (2,0)
equipotentials on
```
````

`charge id ±q at (x,y)`; `wire out|in at (x,y)` (current out of / into the page, circular magnetic field); `uniform angle=0`; `lines 12` (lines per unit charge); `equipotentials on`; `view x1 y1 x2 y2`. Field lines start on positive charges and end on negative ones; arrows show direction.

### circuit

A grid of points; each part sits between two of them. A dot appears where three wires meet.

````
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
````

Parts (`name id (from) (to) ["label"]`): `resistor`, `capacitor`, `inductor`, `battery` (the + terminal is the `to` end), `acsource`, `currentsource`, `switch` (`open` or `closed`), `diode` (anode to cathode, from → to), `led`, `lamp`, `voltmeter`, `ammeter`. Also `wire (a) (b) (c)...`, `ground (x,y)`, `node (x,y)`, `opamp U1 at (x,y) ["label"] [flip]` (inputs at x−2.5, y±0.5; output at x+2.5), `current (a) (b) "I"` (an arrow), `label (x,y) "text"`. Parts are 1.4 units long; leave at least 2 between ends.

### wave

````
```physics
wave "Beats"
wave w1 A=1 lambda=4 f=0.5 "w1"
wave w2 A=1 lambda=3 f=0.5 "w2"
sum w1 w2 "w1 + w2"
xrange 0..12
```
````

`wave id A=1 lambda=2 f=0.5 [phase=0deg] ["label"] [#colour]`: y = A sin(2πx/λ − 2πft + φ). `sum w1 w2` draws the superposition. `standing n=3 L=6 [A=1] [f=0.5]` draws a string fixed at both ends with its nodes. `annotate` marks λ and A on the first wave and prints v = λf and T. `xrange a..b`, `animate off`. Waves play; there is a time slider.

### levels

````
```physics
levels "Hydrogen"
hydrogen n=1..5
transition 3 -> 2 "Hα"
transition 2 -> 1 "Lyman α"
```
````

`level n=1 E=-13.6 ["label"]` for any system (`unit eV` by default, change with `unit kJ/mol`); `hydrogen n=1..5` adds E = −13.6 / n² eV. `transition from -> to ["label"]`: downward is an emission (wavy photon), upward an absorption; ΔE and, in eV, λ = 1239.84 / ΔE nm are printed.

## Checking

`check_diagram kind=plot` evaluates the functions (it warns when one is not a number anywhere on its range). `kind=physics` compiles the figure and returns its notes (image distance, wavelength, transition energies), or the line that is wrong.
