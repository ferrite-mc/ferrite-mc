Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

[System.Windows.Forms.Application]::EnableVisualStyles()

$form = New-Object System.Windows.Forms.Form
$form.Text = 'Ferrite Server'
$form.StartPosition = 'CenterScreen'
$form.Size = New-Object System.Drawing.Size(820, 600)
$form.MinimumSize = New-Object System.Drawing.Size(700, 480)

$root = New-Object System.Windows.Forms.TableLayoutPanel
$root.Dock = 'Fill'
$root.Padding = New-Object System.Windows.Forms.Padding(12)
$root.ColumnCount = 1
$root.RowCount = 4
$root.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Absolute', 76)))
$root.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Absolute', 48)))
$root.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Percent', 100)))
$root.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Absolute', 38)))
$form.Controls.Add($root)

$paths = New-Object System.Windows.Forms.TableLayoutPanel
$paths.Dock = 'Fill'
$paths.ColumnCount = 3
$paths.RowCount = 2
$paths.ColumnStyles.Add((New-Object System.Windows.Forms.ColumnStyle('Percent', 100)))
$paths.ColumnStyles.Add((New-Object System.Windows.Forms.ColumnStyle('Absolute', 90)))
$paths.ColumnStyles.Add((New-Object System.Windows.Forms.ColumnStyle('Absolute', 90)))
$paths.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Absolute', 34)))
$paths.RowStyles.Add((New-Object System.Windows.Forms.RowStyle('Absolute', 34)))
$root.Controls.Add($paths, 0, 0)

$folderBox = New-Object System.Windows.Forms.TextBox
$folderBox.Dock = 'Fill'
$folderBox.Text = (Get-Location).Path
$exeBox = New-Object System.Windows.Forms.TextBox
$exeBox.Dock = 'Fill'
$exeBox.Text = 'ferrite-server.exe'
$chooseFolder = New-Object System.Windows.Forms.Button
$chooseFolder.Text = '서버 폴더'
$chooseExe = New-Object System.Windows.Forms.Button
$chooseExe.Text = '실행 파일'
$paths.Controls.Add($folderBox, 0, 0)
$paths.Controls.Add($chooseFolder, 1, 0)
$paths.SetColumnSpan($chooseFolder, 2)
$paths.Controls.Add($exeBox, 0, 1)
$paths.Controls.Add($chooseExe, 1, 1)
$paths.SetColumnSpan($chooseExe, 2)

$actions = New-Object System.Windows.Forms.FlowLayoutPanel
$actions.Dock = 'Fill'
$actions.FlowDirection = 'LeftToRight'
$root.Controls.Add($actions, 0, 1)
$startButton = New-Object System.Windows.Forms.Button
$startButton.Text = '서버 시작'
$startButton.Width = 110
$startButton.Height = 32
$stopButton = New-Object System.Windows.Forms.Button
$stopButton.Text = '서버 종료'
$stopButton.Width = 110
$stopButton.Height = 32
$stopButton.Enabled = $false
$eulaButton = New-Object System.Windows.Forms.Button
$eulaButton.Text = 'EULA 열기'
$eulaButton.Width = 110
$eulaButton.Height = 32
$openFolderButton = New-Object System.Windows.Forms.Button
$openFolderButton.Text = '폴더 열기'
$openFolderButton.Width = 110
$openFolderButton.Height = 32
$actions.Controls.AddRange(@($startButton, $stopButton, $eulaButton, $openFolderButton))

$logBox = New-Object System.Windows.Forms.TextBox
$logBox.Multiline = $true
$logBox.ReadOnly = $true
$logBox.ScrollBars = 'Vertical'
$logBox.WordWrap = $false
$logBox.Dock = 'Fill'
$logBox.Font = New-Object System.Drawing.Font('Consolas', 9)
$root.Controls.Add($logBox, 0, 2)

$status = New-Object System.Windows.Forms.Label
$status.Dock = 'Fill'
$status.TextAlign = 'MiddleLeft'
$status.Text = '서버 폴더와 실행 파일을 선택하세요.'
$root.Controls.Add($status, 0, 3)

$script:serverProcess = $null
$script:serverFolder = $null

function Add-LogLine([string]$line) {
    if ($form.IsDisposed) { return }
    if ($form.InvokeRequired) {
        [void]$form.BeginInvoke([Action]{ $logBox.AppendText($line + [Environment]::NewLine) })
        return
    }
    $logBox.AppendText($line + [Environment]::NewLine)
}

function Get-SelectedFolder {
    $candidate = $folderBox.Text.Trim()
    if (-not (Test-Path -LiteralPath $candidate -PathType Container)) {
        [System.Windows.Forms.MessageBox]::Show('유효한 서버 폴더를 선택하세요.', 'Ferrite') | Out-Null
        return $null
    }
    return (Resolve-Path -LiteralPath $candidate).Path
}

$chooseFolder.Add_Click({
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.SelectedPath = $folderBox.Text
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { $folderBox.Text = $dialog.SelectedPath }
})
$chooseExe.Add_Click({
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Filter = 'Ferrite server (*.exe)|*.exe|All files (*.*)|*.*'
    if (Test-Path -LiteralPath $folderBox.Text -PathType Container) { $dialog.InitialDirectory = $folderBox.Text }
    if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
        $folderBox.Text = Split-Path -Parent $dialog.FileName
        $exeBox.Text = Split-Path -Leaf $dialog.FileName
    }
})
$openFolderButton.Add_Click({
    $selected = Get-SelectedFolder
    if ($selected) { Start-Process explorer.exe -ArgumentList $selected }
})
$eulaButton.Add_Click({
    $selected = Get-SelectedFolder
    if (-not $selected) { return }
    $eulaPath = Join-Path $selected 'eula.txt'
    if (-not (Test-Path -LiteralPath $eulaPath)) {
        [System.Windows.Forms.MessageBox]::Show('서버를 한 번 실행해 EULA 파일을 만든 뒤, 내용을 읽고 동의한다면 eula=true로 직접 바꾸세요.', 'EULA') | Out-Null
        return
    }
    Start-Process notepad.exe -ArgumentList $eulaPath
})

$startButton.Add_Click({
    if ($script:serverProcess -and -not $script:serverProcess.HasExited) { return }
    $selected = Get-SelectedFolder
    if (-not $selected) { return }
    $executable = Join-Path $selected $exeBox.Text.Trim()
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        [System.Windows.Forms.MessageBox]::Show("실행 파일을 찾을 수 없습니다:`n$executable", 'Ferrite') | Out-Null
        return
    }
    $eulaPath = Join-Path $selected 'eula.txt'
    if (-not (Test-Path -LiteralPath $eulaPath) -or (Get-Content -LiteralPath $eulaPath -Raw) -notmatch '(?m)^\s*eula\s*=\s*true\s*$') {
        [System.Windows.Forms.MessageBox]::Show('EULA를 읽고 동의한 경우에만 eula.txt에서 eula=true로 직접 설정하세요.', 'EULA') | Out-Null
        if (Test-Path -LiteralPath $eulaPath) { Start-Process notepad.exe -ArgumentList $eulaPath }
        return
    }
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $executable
    $info.WorkingDirectory = $selected
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $info
    $process.EnableRaisingEvents = $true
    $process.add_OutputDataReceived([System.Diagnostics.DataReceivedEventHandler]{
        param($sender, $eventArgs)
        if ($eventArgs.Data) { Add-LogLine $eventArgs.Data }
    })
    $process.add_ErrorDataReceived([System.Diagnostics.DataReceivedEventHandler]{
        param($sender, $eventArgs)
        if ($eventArgs.Data) { Add-LogLine $eventArgs.Data }
    })
    $process.add_Exited([System.EventHandler]{
        param($sender, $eventArgs)
        if (-not $form.IsDisposed) {
            [void]$form.BeginInvoke([Action]{
                $startButton.Enabled = $true
                $stopButton.Enabled = $false
                $status.Text = '서버가 종료되었습니다.'
            })
        }
    })
    try {
        [void]$process.Start()
        $process.BeginOutputReadLine()
        $process.BeginErrorReadLine()
        $script:serverProcess = $process
        $script:serverFolder = $selected
        $startButton.Enabled = $false
        $stopButton.Enabled = $true
        $status.Text = "실행 중 · $selected"
        Add-LogLine "[Ferrite] 시작: $executable"
        Add-LogLine "[Ferrite] 작업 폴더: $selected"
    } catch {
        [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, '서버 시작 실패') | Out-Null
    }
})

$stopButton.Add_Click({
    $process = $script:serverProcess
    if (-not $process -or $process.HasExited) { return }
    try {
        $process.StandardInput.WriteLine('stop')
        $process.StandardInput.Flush()
        $status.Text = '정상 종료 및 저장을 기다리는 중…'
        Add-LogLine '[Ferrite] stop 명령을 보냈습니다. 저장이 끝날 때까지 기다립니다.'
        $timer = New-Object System.Windows.Forms.Timer
        $timer.Interval = 10000
        $timer.Add_Tick({
            $timer.Stop()
            $timer.Dispose()
            if ($script:serverProcess -and -not $script:serverProcess.HasExited) {
                Add-LogLine '[Ferrite] 아직 종료되지 않았습니다. 프로세스를 강제 종료하지 않았습니다.'
                $status.Text = '종료 대기 중 · 저장을 위해 프로세스를 유지합니다.'
            }
        })
        $timer.Start()
    } catch {
        Add-LogLine ("[Ferrite] 종료 요청 실패: " + $_.Exception.Message)
    }
})

$form.Add_FormClosing({
    if ($script:serverProcess -and -not $script:serverProcess.HasExited) {
        $_.Cancel = $true
        $status.Text = '서버가 저장 및 종료를 마칠 때까지 창을 열어 둡니다.'
        $stopButton.PerformClick()
    }
})

[void]$form.ShowDialog()
