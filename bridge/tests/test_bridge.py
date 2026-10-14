import asyncio
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import hon_bridge  # noqa: E402


class Param:
    def __init__(self, values=None, lo=None, hi=None):
        self.values, self.min, self.max, self.value = values, lo, hi, None


class Command:
    def __init__(self, params):
        self.parameters, self.sent = params, 0

    async def send(self):
        self.sent += 1
        return True


class FakeAc:
    nick_name, model_name, mac_address, appliance_type, connection = "Tomás AC", "AS25", "aa", "AC", True

    def __init__(self):
        self.data = {"onOffStatus": "0", "machMode": "1", "tempSel": "22.0", "tempIndoor": "29.0", "windSpeed": "5", "windDirectionVertical": "5"}
        self.commands = {
            "settings": Command({
                "onOffStatus": Param(["1"]), "machMode": Param(["0", "1", "2", "4", "6"]),
                "tempSel": Param(lo=16, hi=30), "windSpeed": Param(["1", "2", "3", "5"]),
                "silentSleepStatus": Param(lo=0, hi=1), "windDirectionVertical": Param(["2", "4", "5", "6", "8"]),
            }),
            "stopProgram": Command({}),
        }

    def get(self, key, default=None):
        return self.data.get(key, default)

    async def update(self, force=False):
        pass


def test_describe_maps_hon_values():
    s = hon_bridge.Bridge.describe(FakeAc())
    assert s["power"] is False and s["mode"] == "cool" and s["targetTemp"] == 22
    assert s["fanSpeed"] == "auto" and s["roomTemp"] == 29.0
    assert s["features"] == ["sleep", "airflow"] and s["airflow"] == "auto" and (s["minTemp"], s["maxTemp"]) == (16, 30)


def test_set_sends_settings_and_stop(monkeypatch):
    async def no_sleep(_):
        pass
    monkeypatch.setattr(hon_bridge.asyncio, "sleep", no_sleep)
    ac = FakeAc()
    b = hon_bridge.Bridge()
    b._ac = lambda mac: ac
    asyncio.run(b.set("aa", {"power": True, "mode": "heat", "targetTemp": 24, "fanSpeed": "low", "sleep": True, "airflow": "swing"}))
    p = ac.commands["settings"].parameters
    assert (p["onOffStatus"].value, p["machMode"].value, p["tempSel"].value, p["windSpeed"].value, p["silentSleepStatus"].value, p["windDirectionVertical"].value) == ("1", "4", 24, "3", "1", "8")
    assert ac.commands["settings"].sent == 1
    asyncio.run(b.set("aa", {"power": False}))
    assert ac.commands["stopProgram"].sent == 1
