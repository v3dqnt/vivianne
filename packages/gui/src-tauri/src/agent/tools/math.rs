//! The `math` tool — exact and numerical mathematics, and plots.
//!
//! Language models are unreliable calculators: they get long arithmetic,
//! algebra and calculus wrong in ways that read as confident. This tool gives
//! the agent a real computer-algebra system instead. The model writes a short
//! Python snippet; it runs in an environment the app manages, with SymPy
//! (symbolic maths), NumPy, SciPy (optimisation, integration, ODEs,
//! statistics, linear algebra), mpmath (arbitrary precision) and Matplotlib.
//!
//! ## The environment
//!
//! A private virtualenv at `~/.viv/math-env`, created on first use — with
//! `uv` when it is installed (seconds), otherwise with the system Python's
//! `venv` and `pip`. Nothing is installed into the user's own Python. A
//! marker file records a completed install, so a half-finished one (the app
//! closed mid-download) is simply redone.
//!
//! ## Output
//!
//! Whatever the script prints comes back as text; `tex(expr)` prints an
//! expression as display LaTeX, which the chat renders. Every Matplotlib
//! figure left open when the script ends is saved and returned as an image —
//! to the model when it can see images, and to the user's screen either way.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use base64::Engine as _;
use tokio::io::AsyncReadExt;

use crate::agent::runtime::types::{AgentToolResult, MessageContent};

const PACKAGES: &[&str] = &["sympy", "numpy", "scipy", "mpmath", "matplotlib"];
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 300;
const SETUP_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_OUTPUT_CHARS: usize = 30_000;
const MAX_PLOTS: usize = 6;
const READY_MARKER: &str = ".viv-ready";
/// Pinned, not "whatever is newest": a brand-new Python often has no
/// prebuilt SciPy wheels yet, and then the install compiles from source for
/// a quarter of an hour. uv fetches this version if it isn't installed.
const PYTHON_VERSION: &str = "3.12";

/// Runs before the model's code. Everything a maths session usually starts
/// with, already imported, so a snippet can be two lines.
const PREAMBLE: &str = r#"
import sys, os, math, warnings
warnings.filterwarnings("ignore")
import numpy as np
import sympy as sp
from sympy import *
import mpmath

def tex(expr, inline=False):
    """Print an expression as LaTeX the chat renders."""
    s = expr if isinstance(expr, str) else sp.latex(expr)
    print(f"${s}$" if inline else f"$$\n{s}\n$$")
"#;

/// SciPy, imported only when the snippet mentions it: it's most of the
/// start-up time, and most snippets are pure SymPy.
const SCIPY: &str = r#"
import scipy
# Aliased, not imported by bare name: sympy's star import already defines
# `integrate` and `interpolate`, and the scipy modules would shadow them.
import scipy.optimize as opt
import scipy.integrate as si
import scipy.stats as st
import scipy.special as sps
import scipy.interpolate as spi
import scipy.linalg as sla
"#;

/// Matplotlib, only when the snippet plots. Every figure left open at exit is
/// saved for the caller.
const PLOTTING: &str = r#"
import atexit
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
def __viv_save_figures():
    for i, num in enumerate(plt.get_fignums()[:6]):
        try:
            plt.figure(num).savefig(os.path.join(os.environ["VIV_MATH_OUT"], f"fig{i}.png"), dpi=130, bbox_inches="tight")
        except Exception as e:
            print(f"[could not save figure {i}: {e}]", file=sys.stderr)
atexit.register(__viv_save_figures)
"#;

/// Runs the snippet from its own file, so tracebacks give the snippet's own
/// line numbers and only its frames, and a bare last expression is printed
/// the way a notebook would rather than lost.
const RUNNER: &str = r#"
def __viv_run(path):
    import ast, linecache, traceback
    src = open(path, encoding="utf-8").read()
    linecache.cache["<snippet>"] = (len(src), None, src.splitlines(True), "<snippet>")
    try:
        tree = ast.parse(src, "<snippet>")
        if tree.body and isinstance(tree.body[-1], ast.Expr):
            last = tree.body[-1]
            show = ast.Expr(ast.Call(ast.Name("__viv_show", ast.Load()), [last.value], []))
            tree.body[-1] = ast.copy_location(show, last)
            ast.fix_missing_locations(tree)
        exec(compile(tree, "<snippet>", "exec"), globals())
    except SystemExit:
        raise
    except BaseException as e:
        te = traceback.TracebackException(type(e), e, e.__traceback__)
        te.stack = traceback.StackSummary.from_list([f for f in te.stack if f.filename == "<snippet>"])
        print("".join(te.format()), end="", file=sys.stderr)
        sys.exit(1)

def __viv_show(value):
    if value is not None:
        print(value)

__viv_run(os.environ["VIV_MATH_SNIPPET"])
"#;

/// The script for `code`: the preamble, whatever extras it uses, the runner.
fn script_for(code: &str) -> String {
    let mentions = |words: &[&str]| words.iter().any(|w| code.contains(w));
    let mut s = String::from(PREAMBLE);
    if mentions(&["scipy", "opt.", "si.", "st.", "sps.", "spi.", "sla."]) {
        s.push_str(SCIPY);
    }
    // `plot` also covers sympy's own plot(), which draws with matplotlib.
    if mentions(&["plt", "plot", "matplotlib"]) {
        s.push_str(PLOTTING);
    }
    s.push_str(RUNNER);
    s
}

static SETUP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn env_dir() -> Result<PathBuf, String> {
    Ok(crate::agent::system::paths::viv_home()?.join("math-env"))
}

fn env_python(env: &Path) -> PathBuf {
    if cfg!(windows) {
        env.join("Scripts").join("python.exe")
    } else {
        env.join("bin").join("python")
    }
}

fn command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    {
        // No console window flashing up for every calculation.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

async fn run_setup_step(mut cmd: tokio::process::Command, what: &str) -> Result<(), String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let out = tokio::time::timeout(SETUP_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("{what} timed out"))?
        .map_err(|e| format!("{what} couldn't start: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
        Err(format!("{what} failed:\n{tail}"))
    }
}

/// A Python that can already import everything — the user's own scientific
/// install, if they have one. Found once per process.
async fn existing_python() -> Option<PathBuf> {
    static FOUND: tokio::sync::OnceCell<Option<PathBuf>> = tokio::sync::OnceCell::const_new();
    FOUND
        .get_or_init(|| async {
            let check = format!("import {}", PACKAGES.join(", "));
            for path in crate::coding_agent::checkers::real_pythons() {
                let mut cmd = command(&path);
                cmd.args(["-c", &check]).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
                if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(20), cmd.status()).await {
                    if status.success() {
                        return Some(path);
                    }
                }
            }
            None
        })
        .await
        .clone()
}

/// The Python to run with, in order of preference: an explicit
/// `VIV_MATH_PYTHON`; the app's own environment once it's set up; a system
/// Python that already has every package (no download at all); otherwise a
/// freshly built environment. The `bool` says whether setup just happened.
async fn ensure_env() -> Result<(PathBuf, bool), String> {
    if let Some(p) = std::env::var_os("VIV_MATH_PYTHON").map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok((p, false));
    }
    let env = env_dir()?;
    let python = env_python(&env);
    if env.join(READY_MARKER).is_file() && python.is_file() {
        return Ok((python, false));
    }
    if let Some(p) = existing_python().await {
        return Ok((p, false));
    }
    let _guard = SETUP_LOCK.lock().await;
    // Another call may have finished the install while this one waited.
    if env.join(READY_MARKER).is_file() && python.is_file() {
        return Ok((python, false));
    }
    let _ = std::fs::remove_dir_all(&env);

    if let Ok(uv) = which::which("uv") {
        let mut venv = command(&uv);
        venv.args(["venv", "--quiet", "--python", PYTHON_VERSION]).arg(&env);
        run_setup_step(venv, "Creating the maths environment").await?;
        let mut install = command(&uv);
        // Wheels only: a missing wheel should fail in seconds with a clear
        // error, not start a compiler.
        install.args(["pip", "install", "--only-binary", ":all:", "--python"]).arg(&python).args(PACKAGES);
        run_setup_step(install, "Installing the maths packages").await?;
    } else {
        let system = crate::coding_agent::checkers::python()
            .ok_or_else(|| {
                "The math tool needs Python 3.9+ (or uv). Install it from python.org and try again.".to_string()
            })?;
        let mut venv = command(&system);
        venv.args(["-m", "venv"]).arg(&env);
        run_setup_step(venv, "Creating the maths environment").await?;
        let mut install = command(&python);
        install.args(["-m", "pip", "install", "--quiet", "--disable-pip-version-check", "--only-binary", ":all:"]).args(PACKAGES);
        run_setup_step(install, "Installing the maths packages").await?;
    }
    std::fs::write(env.join(READY_MARKER), PACKAGES.join("\n")).map_err(|e| format!("Couldn't finish setup: {e}"))?;
    Ok((python, true))
}

/// Long output keeps its start and its end: the end is where the answer, or
/// the error, usually is.
fn clip(s: &str) -> String {
    let n = s.chars().count();
    if n <= MAX_OUTPUT_CHARS {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_OUTPUT_CHARS * 2 / 3).collect();
    let tail: String = s.chars().skip(n - MAX_OUTPUT_CHARS / 3).collect();
    format!("{head}\n…[{} characters clipped]…\n{tail}", n - MAX_OUTPUT_CHARS)
}

/// Run the model's snippet. `cwd` is the workspace, so it can read data files.
pub async fn execute(args: &serde_json::Value, cwd: Option<String>, vision: bool) -> AgentToolResult {
    let text_result = |text: String| AgentToolResult {
        content: vec![MessageContent::Text { text }],
        details: serde_json::Value::Null,
        terminate: None,
    };
    let Some(code) = args.get("code").and_then(|v| v.as_str()).filter(|c| !c.trim().is_empty()) else {
        return text_result("Error: 'code' is required.".to_string());
    };
    let timeout = args
        .get("timeout_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);

    let (python, installed_now) = match ensure_env().await {
        Ok(p) => p,
        Err(e) => return text_result(format!("Error: {e}")),
    };

    let work = std::env::temp_dir().join(format!("viv-math-{}", uuid::Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&work) {
        return text_result(format!("Error: couldn't create a scratch folder: {e}"));
    }
    let script = work.join("run.py");
    let snippet = work.join("snippet.py");
    if let Err(e) = std::fs::write(&script, script_for(code)).and_then(|_| std::fs::write(&snippet, code)) {
        let _ = std::fs::remove_dir_all(&work);
        return text_result(format!("Error: couldn't write the snippet: {e}"));
    }

    let mut cmd = command(&python);
    cmd.arg(&script)
        .env("VIV_MATH_OUT", &work)
        .env("VIV_MATH_SNIPPET", &snippet)
        .env("PYTHONIOENCODING", "utf-8")
        .env("MPLBACKEND", "Agg")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = cwd.filter(|d| Path::new(d).is_dir()) {
        cmd.current_dir(dir);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&work);
            return text_result(format!("Error: couldn't start Python: {e}"));
        }
    };
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let read_out = async {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s).await;
        s
    };
    let read_err = async {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s).await;
        s
    };
    let waited = tokio::time::timeout(Duration::from_secs(timeout), async {
        let (out, err, status) = tokio::join!(read_out, read_err, child.wait());
        (out, err, status)
    })
    .await;

    let (out, err, ok) = match waited {
        Ok((out, err, status)) => (out, err, status.map(|s| s.success()).unwrap_or(false)),
        Err(_) => {
            let _ = std::fs::remove_dir_all(&work);
            return text_result(format!(
                "Error: the calculation didn't finish within {timeout}s and was stopped. Simplify it, work numerically, or raise timeout_seconds (max {MAX_TIMEOUT_SECS})."
            ));
        }
    };

    let mut plots: Vec<String> = Vec::new();
    for i in 0..MAX_PLOTS {
        if let Ok(bytes) = std::fs::read(work.join(format!("fig{i}.png"))) {
            plots.push(base64::engine::general_purpose::STANDARD.encode(bytes));
        }
    }
    let _ = std::fs::remove_dir_all(&work);

    let mut text = String::new();
    if installed_now {
        text.push_str("(Set up the maths environment — SymPy, NumPy, SciPy, mpmath, Matplotlib — on first use.)\n\n");
    }
    let out = out.trim_end();
    let err = err.trim_end();
    if !out.is_empty() {
        text.push_str(out);
    }
    if !err.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(if ok { "stderr:\n" } else { "Error:\n" });
        text.push_str(err);
    }
    if text.trim().is_empty() {
        text = if plots.is_empty() {
            "Finished with no output — print() the results you want to see.".to_string()
        } else {
            String::new()
        };
    }
    if !plots.is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!("{} plot{} attached.", plots.len(), if plots.len() == 1 { "" } else { "s" }));
    }
    let text = clip(&text);
    let text = if ok { text } else if text.starts_with("Error") { text } else { format!("Error: {text}") };

    let mut content = vec![MessageContent::Text { text }];
    let details = if vision {
        for b64 in plots {
            content.push(MessageContent::Image { url: None, base64: Some(b64), media_type: Some("image/png".to_string()) });
        }
        serde_json::Value::Null
    } else {
        // A text-only model can't take the image; the user still sees it.
        serde_json::json!({ "plots": plots })
    };
    AgentToolResult { content, details, terminate: None }
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "name": "math",
        "description": "Do real mathematics — use this instead of computing in your head for anything beyond trivial arithmetic. Runs a Python snippet with a computer-algebra system and scientific stack already imported:\n  sympy (as sp, and `from sympy import *`): symbols, simplify, expand, factor, solve, nsolve, diff, integrate, limit, series, dsolve, Matrix, summation, Eq, lambdify\n  numpy as np; scipy as opt=optimize, si=integrate (solve_ivp, quad), st=stats, sps=special, spi=interpolate, sla=linalg — sympy's own integrate()/interpolate() keep their names; mpmath for arbitrary precision; math\n  matplotlib.pyplot as plt — every figure left open is returned as an image, so plot freely\n  tex(expr) prints an expression as display LaTeX (tex(expr, inline=True) for inline)\nprint() whatever you want back; a bare expression on the last line is printed for you. Errors give your snippet's own line numbers. Prefer exact symbolic results, then give a numeric value with evalf()/N(). For models (ODEs, optimisation, fitting, simulation) state the assumptions in comments. The working directory is the project, so data files can be read. The first call sets up the environment and can take a minute.",
        "parameters": {
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "Python source. Print results; use tex(expr) for formulas." },
                "timeout_seconds": { "type": "number", "description": "Max seconds (default 60, max 300)" }
            },
            "required": ["code"]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_named_math_and_requires_code() {
        let s = schema();
        assert_eq!(s["name"], "math");
        assert_eq!(s["parameters"]["required"][0], "code");
    }

    #[test]
    fn preamble_exposes_the_documented_helpers() {
        for name in ["def tex(", "import sympy as sp", "from sympy import *", "import numpy as np"] {
            assert!(PREAMBLE.contains(name), "{name}");
        }
    }

    #[test]
    fn scipy_and_matplotlib_load_only_when_used() {
        let plain = script_for("x = symbols('x')\nsolve(x**2 - 4, x)");
        assert!(!plain.contains("import scipy") && !plain.contains("matplotlib"));
        let both = script_for("sol = si.solve_ivp(f, (0, 1), [1])\nplt.plot(sol.t, sol.y[0])");
        assert!(both.contains("import scipy.integrate as si") && both.contains("matplotlib.use(\"Agg\")"));
        assert!(script_for("plot(sin(x))").contains("atexit.register"), "sympy's plot() draws with matplotlib");
    }

    /// Real end-to-end run: builds the environment (network, a minute or two
    /// the first time), then does symbolic calculus, a numeric ODE and a plot.
    /// `cargo test --lib real_math -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn real_math_run() {
        let code = r#"
x = symbols('x')
tex(integrate(exp(-x**2), (x, -oo, oo)))
print(solve(x**2 - 5*x + 6, x))
sol = si.solve_ivp(lambda t, y: -2*y, (0, 1), [1.0])
print(round(float(sol.y[0][-1]), 4))
plt.plot([0, 1, 2], [0, 1, 4])
factorial(5)
"#;
        let r = execute(&serde_json::json!({ "code": code }), None, true).await;
        let text = match &r.content[0] { MessageContent::Text { text } => text.clone(), _ => String::new() };
        println!("{text}");
        assert!(text.contains(r"\sqrt{\pi}"), "{text}");
        assert!(text.contains("[2, 3]"), "{text}");
        // RK45 at default tolerance: e^-2 = 0.13534 to about three places.
        assert!(text.contains("0.135"), "{text}");
        assert!(text.contains("120"), "a bare last expression is printed: {text}");
        assert_eq!(r.content.iter().filter(|c| matches!(c, MessageContent::Image { .. })).count(), 1, "{text}");
    }

    #[test]
    fn clip_keeps_both_ends_of_long_output() {
        assert_eq!(clip("x = 2"), "x = 2");
        let c = clip(&format!("{}ZeroDivisionError", "a".repeat(MAX_OUTPUT_CHARS)));
        assert!(c.contains("clipped") && c.ends_with("ZeroDivisionError"));
    }

    /// The runner's traceback, against a real Python.
    /// `cargo test --lib real_math -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn real_math_errors_point_at_the_snippet() {
        let r = execute(&serde_json::json!({ "code": "x = 1\ny = x / 0" }), None, false).await;
        let text = match &r.content[0] { MessageContent::Text { text } => text.clone(), _ => String::new() };
        assert!(text.contains("\"<snippet>\", line 2") && text.contains("ZeroDivisionError"), "{text}");
        assert!(!text.contains("__viv_run"), "runner frames are hidden: {text}");
    }
}
