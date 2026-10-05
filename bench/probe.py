"""Debug helper: open a scenario page in a throwaway 1280x800 Chrome and evaluate JS.

    .venv/bin/python probe.py /booking-calendar/site/index.html "expr" ["expr2" ...]
"""
import json, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from chrome import Chrome, Cdp
from server import BenchServer
from tasks import EXAMPLES

server = BenchServer(EXAMPLES, 0).start()
chrome = Chrome(9444, headless=True).launch(server.base + sys.argv[1])
try:
    cdp = Cdp(chrome.first_page()["webSocketDebuggerUrl"])
    time.sleep(1.0)
    for expr in sys.argv[2:]:
        if expr.startswith("sleep:"):
            time.sleep(float(expr[6:])); continue
        res = cdp.call("Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True)
        print(json.dumps(res.get("result", {}).get("value", res.get("exceptionDetails", {}).get("text")), default=str)[:4000])
finally:
    chrome.kill(); server.stop()
