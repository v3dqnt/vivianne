import type { SUBAGENT_COLORS } from '../../settings/lib/subagentProfiles'

type Color = (typeof SUBAGENT_COLORS)[number]

/** One agent of a starter team, before it's saved to `.viv/agents`. */
export interface StarterAgent {
  name: string
  role: string
  color: Color
  instructions: string
}

export interface StarterTeam {
  id: string
  label: string
  blurb: string
  agents: StarterAgent[]
}

const SOFTWARE: StarterTeam = {
  id: 'software',
  label: 'Build team',
  blurb: 'Architect, Builder, Reviewer',
  agents: [
    {
      name: 'Architect',
      role: 'Designs the structure: modules, interfaces, data shapes, and how the pieces fit.',
      color: 'blue',
      instructions:
        'Decide the shape of the work before code is written: which modules, which interfaces between them, which data flows where. Tell the Builder the decisions they need, and write them down in the project. Push back when a request would tangle the design.',
    },
    {
      name: 'Builder',
      role: 'Implements the work, following the agreed design.',
      color: 'green',
      instructions:
        'Implement what the task asks, in the design the Architect set out — ask them (message_agent) when it is unclear rather than guessing. Keep changes focused. Build and run what you change before reporting.',
    },
    {
      name: 'Reviewer',
      role: 'Tests and reviews what is built: bugs, edge cases, and whether it does what was asked.',
      color: 'amber',
      instructions:
        'Check the work as it lands: run it, write or run tests for the core behaviour and the edge cases, and read the diff for bugs. Tell the Builder concretely what is wrong and where. Your report says what was verified and how.',
    },
  ],
}

const MODELLING: StarterTeam = {
  id: 'modelling',
  label: 'Modelling team',
  blurb: 'Modeller, Numerical analyst, Critic',
  agents: [
    {
      name: 'Modeller',
      role: 'Builds the mathematical model: variables, assumptions, equations, and their justification.',
      color: 'purple',
      instructions:
        'Formulate the model from first principles or the literature: state every variable with units, every assumption and why it is reasonable, and derive the governing equations. Non-dimensionalise and identify the key parameters and regimes. Give the Numerical analyst an exact statement of what to solve.',
    },
    {
      name: 'Numerical analyst',
      role: 'Solves the model: analytical results where possible, numerics otherwise, with checked accuracy.',
      color: 'teal',
      instructions:
        'Solve what the Modeller specifies. Prefer exact or asymptotic results where they exist; otherwise implement a numerical method suited to the problem, and show it converges (refine the step or grid, compare against a known case). Produce the figures, with every script needed to regenerate them.',
    },
    {
      name: 'Critic',
      role: 'Checks the work: units, limiting cases, assumptions, and agreement with known results and data.',
      color: 'red',
      instructions:
        'Try to break the model and its results. Check dimensional consistency, limiting and special cases, conservation laws, sensitivity to the assumptions, and agreement with published results or data. Tell the others exactly what fails and why. Your report lists what holds up and what does not.',
    },
  ],
}

const RESEARCH: StarterTeam = {
  id: 'research',
  label: 'Research team',
  blurb: 'Literature scout, Analyst, Writer',
  agents: [
    {
      name: 'Literature scout',
      role: 'Finds and summarises the relevant prior work, methods and data sources, with citations.',
      color: 'blue',
      instructions:
        'Search for the prior work that bears on the question: key papers, established methods, available datasets, open disagreements. Summarise what is known and cite every source with enough detail to find it. Point the Analyst at the methods and data worth using.',
    },
    {
      name: 'Analyst',
      role: 'Designs and runs the analysis: data preparation, methods, statistics, and figures.',
      color: 'green',
      instructions:
        'Carry out the analysis the question needs: prepare the data, choose methods that fit it (justify the choice), check their assumptions, and report effect sizes and uncertainty, not just p-values. Keep everything in reproducible scripts or notebooks.',
    },
    {
      name: 'Writer',
      role: 'Turns the findings into clear writing and figures, with honest limitations.',
      color: 'pink',
      instructions:
        'Write up what the team found: the question, the method, the results and their uncertainty, and the limitations — in plain, precise language. Make the figures readable on their own. Ask the Analyst about anything you cannot state accurately.',
    },
  ],
}

const SIMULATION: StarterTeam = {
  id: 'simulation',
  label: 'Simulation team',
  blurb: 'Modeller, Simulation engineer, Validator',
  agents: [
    MODELLING.agents[0],
    {
      name: 'Simulation engineer',
      role: 'Builds and runs the simulation: correct, efficient, and instrumented.',
      color: 'teal',
      instructions:
        'Implement the simulation of what the Modeller specifies, with the right numerical scheme and time step, and instrument it to record what the question needs. Make it fast enough to run the parameter sweeps required. Keep runs reproducible (seeds, configs, versions).',
    },
    {
      name: 'Validator',
      role: 'Validates the simulation against theory and data, and quantifies its error.',
      color: 'amber',
      instructions:
        'Show the simulation is right: convergence under refinement, agreement with analytical cases and conserved quantities, and comparison with experimental or published data. Quantify the error and say where the simulation can and cannot be trusted.',
    },
  ],
}

/** Starter teams offered for a kind of project, best fit first. */
export function starterTeamsFor(kind: string | null): StarterTeam[] {
  switch (kind) {
    case 'Mathematical modelling':
      return [MODELLING, SIMULATION, RESEARCH]
    case 'Simulation':
      return [SIMULATION, MODELLING]
    case 'Scientific research':
    case 'Data analysis / statistics':
      return [RESEARCH, MODELLING]
    default:
      return [SOFTWARE, RESEARCH]
  }
}
