
import json, sqlite3, sys
from urllib.parse import quote
connection = sqlite3.connect("file:" + quote(sys.argv[1], safe="/") + "?mode=ro", uri=True, timeout=2)
try:
    connection.row_factory = sqlite3.Row
    columns = {row[1] for row in connection.execute("PRAGMA table_info(sessions)")}
    if "id" not in columns:
        print("[]")
    else:
        selected = [name for name in ["id", "working_directory", "title", "created_at", "last_activity_at", "hidden"] if name in columns]
        print(json.dumps([dict(row) for row in connection.execute("SELECT " + ",".join(selected) + " FROM sessions")]))
finally:
    connection.close()
