# skill:file_reading

## Model
gpt-oss:120b-cloud

## Role
Read, inspect, and extract content from uploaded or referenced files. Route to the correct tool for each file type instead of blindly reading binary data.

## Dispatch Table

| Extension | Approach | Library |
|---|---|---|
| `.pdf` | `pdfplumber` or `pypdf` | `pdfplumber`, `pypdf` |
| `.docx` | `python-docx` | `docx` |
| `.xlsx` | `openpyxl` or `pandas` | `openpyxl`, `pandas` |
| `.csv`, `.tsv` | `pandas` with `nrows` | `pandas` |
| `.json` | `json.load` + inspect structure | `json` |
| `.png`, `.jpg` | Already visual; use PIL for processing | `Pillow` |
| `.zip`, `.tar` | List contents only, don't auto-extract | `zipfile`, `tarfile` |
| `.txt`, `.md`, `.log` | Check size first, then `head` | built-in |

## File Reading Strategies

### PDF
```python
import pdfplumber
with pdfplumber.open("document.pdf") as pdf:
    for page in pdf.pages:
        text = page.extract_text()
        tables = page.extract_tables()
```

### DOCX
```python
from docx import Document
doc = Document("file.docx")
for para in doc.paragraphs:
    print(para.style.name, ":", para.text)
```

### XLSX
```python
from openpyxl import load_workbook
wb = load_workbook("file.xlsx", read_only=True)
ws = wb.active
for row in ws.iter_rows(max_row=5, values_only=True):
    print(row)
```

### CSV
```python
import pandas as pd
df = pd.read_csv("file.csv", nrows=5)
print(df)
print(df.dtypes)
```

## Rules
- **Stat before reading** — check file size first
- **Read just enough** to answer the question
- **Never cat binary files** — use the correct library
- For large files, sample with `nrows` or `head`
- For images, process with PIL if computation needed
