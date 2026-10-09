# Biology

One fence, ` ```bio `, five kinds of figure. The first line names the kind (and an optional title); the rest are `key: value` lines. Every error comes back with its line number; run non-trivial scripts through `check_diagram kind=bio` first.

Conventions: nucleotide positions are 1-based and inclusive; gene names in italics are not possible in a figure, so write them as the literature does (`lacZα`, `AmpR`); pathway diagrams follow **SBGN** shapes; taxon names in trees use underscores or a quoted string for spaces.

## `sequence`

DNA, RNA or protein, in rows with a position ruler, optional feature bars, and translation lanes.

````
```bio
sequence
name: GFP start
seq: ATGGTGAGCAAGGGCGAGGAGCTGTTCACCGGGGTGGTGCCCATCCTGGTCGAGCTGGACGGC
feature: 1-33 "N-terminal" #3987e5
feature: 34-60 "β-strand 1" #1baf7a
translate: 1
```
````

| Line | Meaning |
|---|---|
| `name:` / `title:` | heading (or put it on the first line: `sequence "GFP start"`) |
| `seq:` | the sequence; repeat the line to continue; spaces, digits and `>FASTA` header lines are ignored |
| `type: dna \| rna \| protein` | detected automatically; say it for short or unusual sequences |
| `feature: 10-45 "label" #colour` | a bar under those positions; overlapping features stack |
| `translate: 1` | reading frames to translate: `1 2 3` forward, `-1 -2 -3` reverse strand; M is green, stop is red |
| `color: none` | turn off base colouring (A green, C blue, G amber, T/U red; protein by class) |
| `width: 60` | residues per row (10 to 200) |
| `complement: on` | show the complementary strand |
| `numbers: off` | hide position numbers |

Use only sequences you know (a real gene, a textbook example) and say when one is illustrative.

## `plasmid`

A circular map: features as arrows around the ring, restriction sites outside with their positions.

````
```bio
plasmid pUC19
size: 2686
feature: 146..469 "lacZα" cw #3987e5
feature: 1626..2486 "AmpR" ccw #e66767
feature: 867..1455 "ori" cw #1baf7a
site: EcoRI 396
site: HindIII 447
```
````

`feature: start..end "label" [cw|ccw|none] [#colour]` — direction is the strand: `cw` clockwise (forward), `ccw` the reverse strand, `none` a plain block. A feature may wrap the origin (`start` greater than `end`). `site: Name position`. Positions must lie inside `size`.

## `tree`

A phylogeny from a Newick string. Branch lengths are drawn when present.

````
```bio
tree "Primates"
newick: (((Human:0.006,Chimp:0.007):0.002,Gorilla:0.009):0.009,Orangutan:0.018);
layout: rectangular
highlight: Human, Chimp, Gorilla "Great apes" #e66767
```
````

| Line | Meaning |
|---|---|
| `newick:` | the tree; may continue over several lines until the `;`. A bare Newick line also works. Internal names that are numbers are drawn as support values |
| `layout:` | `rectangular` (default), `circular`, or `cladogram` (ignores lengths, tips aligned) |
| `scale: off` | hide the scale bar |
| `highlight: A, B "label" #colour` | shades the smallest clade containing those taxa |

Only draw relationships you know are accepted. A topology you made up is a cartoon: say so.

## `pathway`

SBGN-style process diagram: nodes, compartments, and arcs, laid out automatically.

````
```bio
pathway Glycolysis (excerpt)
compartment cyt "Cytosol"
node glc metabolite "Glucose" in cyt
node g6p metabolite "G6P" in cyt
node HK protein "Hexokinase" in cyt
node PFK1 protein "PFK-1" in cyt
node f6p metabolite "F6P" in cyt
node cit metabolite "Citrate" in cyt
glc -> g6p by HK
g6p -> f6p
cit inhibits PFK1
g6p inhibits HK
```
````

**Nodes** — `node id type "Label" [in compartment]`:

| Type | Drawn as (SBGN) |
|---|---|
| `metabolite` | ellipse (simple chemical) |
| `protein` | rounded rectangle (macromolecule) |
| `gene` | rectangle with rounded bottom (nucleic acid feature) |
| `complex` | octagon (complex) |

`compartment id "Label"` draws a dashed region around the nodes that say `in id`. Declare every id before using it.

**Arcs**:

| Line | Meaning | Drawn as |
|---|---|---|
| `A -> B` | conversion | process square, filled arrow |
| `A + B -> C` | several substrates or products | same, joined at the square |
| `A <-> B` | reversible conversion | arrowheads at both ends |
| `A -> B by E` or `E catalyses A -> B` | catalysed conversion | open circle on the enzyme arc |
| `A activates B` | stimulation | open arrowhead |
| `A inhibits B` | inhibition | bar |
| `A transports B` | transport between places (B is the same species at its destination) | dashed arrow |

Add a quoted text to label an arc. `direction: right` lays the pathway out left to right.

## `cell`

A clean illustration of an animal, plant or bacterial cell, labelled down both sides.

````
```bio
cell animal "Animal cell"
highlight: mitochondrion
label: mitochondrion "Makes ATP by oxidative phosphorylation"
```
````

`cell animal | plant | bacterium`. `show:` replaces the default list; `hide:` removes some; `highlight:` marks organelles with a halo; `label: organelle "note"` replaces the small caption; `labels: off` removes all labels.

| Cell | Organelles |
|---|---|
| animal | nucleus, nucleolus, mitochondrion, rough-er, smooth-er, golgi, lysosome, centrioles, ribosome, vesicle, peroxisome, cytoskeleton |
| plant | cell-wall, nucleus, nucleolus, chloroplast, mitochondrion, vacuole, rough-er, smooth-er, golgi, ribosome, vesicle, peroxisome, cytoskeleton |
| bacterium | capsule, cell-wall, nucleoid, plasmid, ribosome, flagellum, pili, vesicle |

(Plural and long forms such as `mitochondria`, `golgi-apparatus`, `er` are understood.) The drawing is schematic: say nothing about relative sizes.
