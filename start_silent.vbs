Set WshShell = CreateObject("WScript.Shell")
WshShell.CurrentDirectory = "C:\Users\kimba\tools\groq-flow"
WshShell.Run """C:\Users\kimba\tools\groq-flow\.venv\Scripts\pythonw.exe"" -m groq_flow.main", 0, False
