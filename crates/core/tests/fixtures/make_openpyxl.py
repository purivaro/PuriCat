import sys, datetime
from openpyxl import Workbook
wb = Workbook()
ws = wb.active
ws.title = "ลูกค้า"
ws.append(["id", "name", "joined", "seen", "opens", "balance", "active", "share", "note"])
ws.append([1, 'สมชาย "ใจดี"', datetime.date(2026, 1, 31), datetime.datetime(2026, 1, 31, 13, 45, 7), datetime.time(8, 30), 1234.5, True, 0.25, None])
ws.append([2, "Ananya Wong", datetime.date(2024, 2, 29), None, None, -0.1, False, 1, "two\nlines"])
ws.append([9007199254740993, "big id as text? no: a number", None, None, None, 0.1 + 0.2, None, None, "007"])
ws["H2"].number_format = "0%"
ws["F2"].number_format = "#,##0.00"
other = wb.create_sheet("Empty")
wb.save(sys.argv[1])
