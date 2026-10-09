# Science in chat

Chemistry, biology, physics and mathematics are drawn, not described. Write a short plain-text script in a fenced block and the chat renders it as a figure: a structure, a reaction, a 3D protein, a plasmid map, a graph with sliders, a circuit. The reader can toggle to the source, copy the figure as SVG or PNG, and open it larger.

| Guide | Fences and inline syntax |
|---|---|
| [chemistry](chemistry.md) | `$\ce{...}$`, `$\pu{...}$`, ` ```chem ` (structures, reactions, mechanisms), ` ```mol3d ` (3D molecules and proteins) |
| [biology](biology.md) | ` ```bio ` with `sequence`, `plasmid`, `tree`, `pathway`, `cell` |
| [physics](physics.md) | ` ```plot ` (graphs with sliders), ` ```physics ` (mechanics, optics, fields, circuits, waves, energy levels) |

## Rules that apply to every fence

1. **Draw it when a picture says it faster.** A structure, a mechanism, a circuit, a free-body diagram, a graph of a function. Do not draw what a sentence says just as well.
2. **Check before you send.** Call `check_diagram` with the `kind` (`chem`, `mol3d`, `bio`, `plot`, `physics`) and the script. It runs the same parser the chat uses and returns either `OK` (for chemistry, with the canonical SMILES, formula and molecular weight of what was drawn) or `INVALID (line N)` with the line quoted. Fix and check again. Confirm the formula and weight match the compound you meant: a valid SMILES for the wrong molecule is the commonest mistake.
3. **Explain next to the figure.** One or two sentences saying what to look at. The figure carries the detail; the text carries the point.
4. **One idea per figure.** Several small figures read better than one crowded one.
5. **Units are SI** unless the field uses something else by convention (eV, bp, kcal/mol). Write the unit in the label: `x: Time t [s]`.
6. **Maths in labels.** `$...$` renders with KaTeX wherever a label is shown. `\alpha`, `\omega`, `->`, `<=>`, `x^2`, `H_2O` also work in labels and are turned into Unicode in copied SVG/PNG.
7. **Never fabricate data.** Invent no measurements, structures or sequences you do not know. Say a sequence or SMILES is illustrative when it is.
8. **No network at draw time** except `mol3d: pdb: ID`, which fetches from RCSB. Everything else is drawn offline.
9. **Accessibility.** Colours are chosen to be distinguishable; do not rely on colour alone, and label every series, feature and force.
