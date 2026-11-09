// Peak working set of native cells on Windows, where Hyperfine records no memory. Node has no
// API for a child's peak memory there, so one PowerShell 7 process reads it through
// GetProcessMemoryInfo after each run, while the handle of the finished process stays open.
// A plan lists cells as the suite times them: program, arguments, working directory, optional
// stdin file, optional state reset (seed, target) before each start, and environment overrides.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const SCRIPT = String.raw`$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PeakMemory {
  [StructLayout(LayoutKind.Sequential)]
  public struct Counters {
    public uint cb; public uint PageFaultCount;
    public UIntPtr PeakWorkingSetSize; public UIntPtr WorkingSetSize;
    public UIntPtr QuotaPeakPagedPoolUsage; public UIntPtr QuotaPagedPoolUsage;
    public UIntPtr QuotaPeakNonPagedPoolUsage; public UIntPtr QuotaNonPagedPoolUsage;
    public UIntPtr PagefileUsage; public UIntPtr PeakPagefileUsage;
  }
  [DllImport("psapi.dll", SetLastError = true)]
  static extern bool GetProcessMemoryInfo(IntPtr process, out Counters counters, uint size);
  // Valid after exit while the handle stays open: the counters of the finished process.
  public static ulong Peak(IntPtr process) {
    Counters counters;
    if (!GetProcessMemoryInfo(process, out counters, (uint)Marshal.SizeOf(typeof(Counters))))
      throw new System.ComponentModel.Win32Exception();
    return counters.PeakWorkingSetSize.ToUInt64();
  }
}
'@

function Invoke-Measured($program, [string[]] $arguments, $directory, $inputFile, $environment) {
  $start = [System.Diagnostics.ProcessStartInfo]::new($program)
  foreach ($argument in $arguments) { $start.ArgumentList.Add($argument) }
  $start.WorkingDirectory = $directory
  $start.UseShellExecute = $false
  $start.RedirectStandardInput = $true
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  if ($environment) { foreach ($entry in $environment.PSObject.Properties) { if ($null -ne $entry.Value) { $start.Environment[$entry.Name] = [string]$entry.Value } } }
  $process = [System.Diagnostics.Process]::Start($start)
  $handle = $process.Handle
  $stdout = $process.StandardOutput.ReadToEndAsync()
  $stderr = $process.StandardError.ReadToEndAsync()
  if ($inputFile) { $process.StandardInput.Write([System.IO.File]::ReadAllText($inputFile)) }
  $process.StandardInput.Close()
  $process.WaitForExit()
  [void]$stdout.Result; $errors = $stderr.Result
  if ($process.ExitCode -ne 0) { throw "$program $($arguments -join ' ') exited $($process.ExitCode): $errors" }
  $peak = [PeakMemory]::Peak($handle)
  $process.Dispose()
  return $peak
}

$planned = Get-Content -Raw $env:CRAFT_MEMORY_PLAN | ConvertFrom-Json
$report = [ordered]@{}
foreach ($cell in $planned.cells) {
  $peaks = @()
  for ($run = 0; $run -lt $planned.runs; $run++) {
    if ($cell.reset) { & $planned.node $planned.reset $cell.reset[0] $cell.reset[1]; if ($LASTEXITCODE -ne 0) { throw "reset failed for $($cell.name)" } }
    $peaks += Invoke-Measured $cell.program @($cell.args) $cell.cwd $cell.input $cell.env
  }
  $report[$cell.name] = $peaks
}
$report | ConvertTo-Json -Depth 3 | Set-Content -Encoding utf8 $env:CRAFT_MEMORY_OUTPUT`;

// Returns { "<cell>": [peakBytes, ...] } or throws with PowerShell's error.
export function peakMemory({ cells, runs, reset, workspace, env = process.env }) {
  const plan = path.join(workspace, 'memory-plan.json'), output = path.join(workspace, 'memory-peaks.json');
  // Run from a file: script text read from stdin is taken line by line and breaks multi-line blocks.
  const script = path.join(workspace, 'peak-memory.ps1');
  fs.writeFileSync(plan, JSON.stringify({ runs, node: process.execPath, reset, cells }));
  fs.writeFileSync(script, SCRIPT);
  const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-File', script], { encoding: 'utf8',
    env: { ...env, CRAFT_MEMORY_PLAN: plan, CRAFT_MEMORY_OUTPUT: output }, maxBuffer: 16 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`pwsh exited ${result.status}: ${(result.stderr || result.stdout).trim()}`);
  return JSON.parse(fs.readFileSync(output, 'utf8'));
}
