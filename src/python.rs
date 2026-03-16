//! Subprocess interface to ai3d-cad Python package.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ModelMetadata {
    pub dimensions: Dimensions,
    pub volume_mm3: f64,
    pub triangle_count: u64,
    pub features: Vec<String>,
    pub watertight: bool,
    pub engine: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Dimensions {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct BuildError {
    pub error: String,
    pub error_type: String,
}

#[derive(Debug)]
pub enum BuildResult {
    Success(ModelMetadata),
    BuildError(BuildError),
    SyntaxError(BuildError),
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Engine {
    CadQuery,
    OpenSCAD,
}

impl Engine {
    pub fn as_str(&self) -> &str {
        match self {
            Engine::CadQuery => "cadquery",
            Engine::OpenSCAD => "openscad",
        }
    }

    pub fn file_extension(&self) -> &str {
        match self {
            Engine::CadQuery => "py",
            Engine::OpenSCAD => "scad",
        }
    }
}

pub fn check_python(python: &str) -> Result<(), String> {
    let output = Command::new(python)
        .args(["-m", "ai3d_cad", "--version"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run {python}: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "ai3d-cad not installed. Run: cd python && {python} -m pip install -e ."
        ));
    }

    let version_str = String::from_utf8_lossy(&output.stdout);
    if !version_str.contains("protocol 1") {
        return Err(format!(
            "Incompatible ai3d-cad version: {}. Expected protocol 1.",
            version_str.trim()
        ));
    }

    Ok(())
}

fn run_python_subprocess(python: &str, args: &[String], timeout: Duration) -> BuildResult {
    let mut child = match Command::new(python)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return BuildResult::BuildError(BuildError {
                error: format!("Failed to spawn python: {e}"),
                error_type: "build".to_string(),
            });
        }
    };

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => {
            let stdout = {
                let mut s = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = io::Read::read_to_string(&mut out, &mut s);
                }
                s
            };

            match status.code() {
                Some(0) => {
                    match serde_json::from_str::<ModelMetadata>(&stdout) {
                        Ok(meta) => BuildResult::Success(meta),
                        Err(e) => BuildResult::BuildError(BuildError {
                            error: format!("Failed to parse metadata: {e}"),
                            error_type: "build".to_string(),
                        }),
                    }
                }
                Some(2) => {
                    match serde_json::from_str::<BuildError>(&stdout) {
                        Ok(err) => BuildResult::SyntaxError(err),
                        Err(_) => BuildResult::SyntaxError(BuildError {
                            error: stdout,
                            error_type: "syntax".to_string(),
                        }),
                    }
                }
                _ => {
                    match serde_json::from_str::<BuildError>(&stdout) {
                        Ok(err) => BuildResult::BuildError(err),
                        Err(_) => BuildResult::BuildError(BuildError {
                            error: stdout,
                            error_type: "build".to_string(),
                        }),
                    }
                }
            }
        }
        Ok(None) => {
            #[cfg(unix)]
            {
                unsafe { libc::kill(child.id() as i32, libc::SIGTERM); }
                std::thread::sleep(Duration::from_secs(5));
                let _ = child.kill();
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
            let _ = child.wait();
            BuildResult::Timeout
        }
        Err(e) => {
            let _ = child.kill();
            BuildResult::BuildError(BuildError {
                error: format!("Wait failed: {e}"),
                error_type: "build".to_string(),
            })
        }
    }
}

pub fn build(
    python: &str,
    code_path: &Path,
    output_path: &Path,
    engine: Engine,
    timeout: Duration,
) -> BuildResult {
    let args = vec![
        "-m".to_string(),
        "ai3d_cad".to_string(),
        "build".to_string(),
        "--code".to_string(),
        code_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_path.to_string_lossy().into_owned(),
        "--engine".to_string(),
        engine.as_str().to_string(),
    ];
    run_python_subprocess(python, &args, timeout)
}

pub fn assemble(
    python: &str,
    manifest_path: &Path,
    output_path: &Path,
    step_path: Option<&Path>,
    timeout: Duration,
) -> BuildResult {
    let mut args = vec![
        "-m".to_string(),
        "ai3d_cad".to_string(),
        "assemble".to_string(),
        "--manifest".to_string(),
        manifest_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_path.to_string_lossy().into_owned(),
    ];
    if let Some(step) = step_path {
        args.push("--step".to_string());
        args.push(step.to_string_lossy().into_owned());
    }
    run_python_subprocess(python, &args, timeout)
}

pub fn paramset(
    python: &str,
    code_path: &Path,
    params_path: &Path,
    output_path: &Path,
    step_path: Option<&Path>,
    timeout: Duration,
) -> BuildResult {
    let mut args = vec![
        "-m".to_string(),
        "ai3d_cad".to_string(),
        "paramset".to_string(),
        "--code".to_string(),
        code_path.to_string_lossy().into_owned(),
        "--params".to_string(),
        params_path.to_string_lossy().into_owned(),
        "--output".to_string(),
        output_path.to_string_lossy().into_owned(),
    ];
    if let Some(step) = step_path {
        args.push("--step".to_string());
        args.push(step.to_string_lossy().into_owned());
    }
    run_python_subprocess(python, &args, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_str() {
        assert_eq!(Engine::CadQuery.as_str(), "cadquery");
        assert_eq!(Engine::OpenSCAD.as_str(), "openscad");
        assert_eq!(Engine::CadQuery.file_extension(), "py");
        assert_eq!(Engine::OpenSCAD.file_extension(), "scad");
    }
}
