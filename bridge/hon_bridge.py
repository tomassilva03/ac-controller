"""Ponte entre a app e o hOn.

A app arranca este programa uma vez e fala com ele por stdin/stdout, uma linha
JSON por pedido e uma por resposta. A sessão do hOn fica aberta enquanto a app
estiver a correr, por isso cada comando só faz o pedido do próprio comando.

Pedidos:
  {"id": 1, "cmd": "login", "email": "...", "password": "..."}
  {"id": 2, "cmd": "state", "ac": "<mac>"}
  {"id": 3, "cmd": "set", "ac": "<mac>", "patch": {"power": true, "targetTemp": 23}}
Respostas:
  {"id": 1, "ok": true, "result": ...}  ou  {"id": 1, "ok": false, "error": "..."}
"""

import asyncio
import json
import logging
import os
import sys
from typing import Any, Dict, Optional

import certifi

# O Python empacotado no macOS não usa os certificados do sistema, por isso as
# ligações HTTPS ao hOn falhavam ("unable to get local issuer certificate").
# Usa-se a lista de certificados do certifi, que vai dentro do executável.
os.environ.setdefault("SSL_CERT_FILE", certifi.where())

from pyhon import Hon  # noqa: E402
from pyhon.exceptions import ApiError, HonAuthenticationError  # noqa: E402

# Valores do hOn para o machMode do AC.
MODES = {"auto": "0", "cool": "1", "dry": "2", "heat": "4", "fan": "6"}
# windSpeed: 1 alta, 2 média, 3 baixa, 5 automática.
FAN_SPEEDS = {"high": "1", "mid": "2", "low": "3", "auto": "5"}
# windDirectionVertical: 8 oscila, 5 automático, 2/4/6 posições fixas de cima para baixo.
AIRFLOW = {"swing": "8", "auto": "5", "top": "2", "middle": "4", "bottom": "6"}
# Funções que só existem nalguns modelos: nome na app -> parâmetro do hOn.
SWITCHES = {"eco": "ecoMode", "sleep": "silentSleepStatus"}


_LOGGER = logging.getLogger("hon_bridge")


def reverse(mapping: Dict[str, str], value: Any, default: str) -> str:
    key = str(int(float(value))) if str(value).replace(".", "", 1).isdigit() else str(value)
    return next((k for k, v in mapping.items() if v == key), default)


def as_bool(value: Any) -> bool:
    try:
        return int(float(value)) == 1
    except (TypeError, ValueError):
        return False


class Bridge:
    def __init__(self) -> None:
        self.hon: Optional[Hon] = None

    async def close(self) -> None:
        if self.hon:
            await self.hon.close()
            self.hon = None

    def _acs(self):
        if not self.hon:
            raise RuntimeError("not_logged_in")
        return [a for a in self.hon.appliances if a.appliance_type == "AC"]

    def _ac(self, mac: str):
        for ac in self._acs():
            if ac.mac_address == mac:
                return ac
        raise RuntimeError("ac_not_found")

    @staticmethod
    async def _ensure_commands(ac) -> None:
        """O hOn às vezes falha ao devolver os comandos no login; tenta de novo
        algumas vezes, porque costuma responder bem passados uns segundos."""
        for attempt in range(4):
            if "settings" in ac.commands:
                return
            if attempt:
                await asyncio.sleep(3)
            _LOGGER.warning("%s sem comandos (%s); a recarregar", ac.nick_name, list(ac.commands))
            try:
                await ac.load_commands()
            except Exception:  # pylint: disable=broad-except
                _LOGGER.exception("load_commands falhou para %s", ac.nick_name)
        if "settings" not in ac.commands:
            raise RuntimeError("no_commands")

    async def login(self, email: str, password: str) -> Any:
        await self.close()
        self.hon = await Hon(email, password).create()
        for ac in self._acs():
            _LOGGER.info("AC %s (%s): comandos %s", ac.nick_name, ac.model_name, list(ac.commands))
        return [
            {"id": a.mac_address, "name": a.nick_name, "model": a.model_name}
            for a in self._acs()
        ]

    @staticmethod
    def describe(ac) -> Dict[str, Any]:
        settings = ac.commands.get("settings")
        params = settings.parameters if settings else {}
        state = {
            "name": ac.nick_name,
            "online": bool(ac.connection),
            "power": as_bool(ac.get("onOffStatus")),
            "mode": reverse(MODES, ac.get("machMode", "1"), "cool"),
            "targetTemp": int(float(ac.get("tempSel", 22) or 22)),
            "roomTemp": float(ac.get("tempIndoor", 0) or 0),
            "outdoorTemp": float(ac.get("tempOutdoor", 0) or 0),
            "fanSpeed": reverse(FAN_SPEEDS, ac.get("windSpeed", "5"), "auto"),
            "airflow": reverse(AIRFLOW, ac.get("windDirectionVertical", "5"), "auto"),
            "features": [k for k, p in {**SWITCHES, "airflow": "windDirectionVertical"}.items() if p in params],
        }
        for key, param in SWITCHES.items():
            state[key] = as_bool(ac.get(param, 0))
        if temp := params.get("tempSel"):
            state["minTemp"], state["maxTemp"] = int(temp.min), int(temp.max)
        return state

    async def state(self, mac: str) -> Any:
        ac = self._ac(mac)
        await self._ensure_commands(ac)
        await ac.update(force=True)
        return self.describe(ac)

    async def set(self, mac: str, patch: Dict[str, Any]) -> Any:
        ac = self._ac(mac)
        await self._ensure_commands(ac)
        await ac.update(force=True)
        if patch.get("power") is False:
            # O hOn responde muitas vezes com erro ao stopProgram, mas o AC
            # desliga-se na mesma uns segundos depois.
            try:
                await ac.commands["stopProgram"].send()
            except ApiError:
                _LOGGER.warning("stopProgram devolveu erro; o AC costuma desligar-se na mesma")
            await asyncio.sleep(1.5)
            await ac.update(force=True)
            return {**self.describe(ac), "power": False}
        else:
            values: Dict[str, Any] = {}
            if patch.get("power"):
                values["onOffStatus"] = "1"
            if "mode" in patch:
                values["machMode"] = MODES[patch["mode"]]
            if "targetTemp" in patch:
                values["tempSel"] = int(patch["targetTemp"])
            if "fanSpeed" in patch:
                values["windSpeed"] = FAN_SPEEDS[patch["fanSpeed"]]
            if "airflow" in patch:
                values["windDirectionVertical"] = AIRFLOW[patch["airflow"]]
            for key, param in SWITCHES.items():
                if key in patch:
                    values[param] = "1" if patch[key] else "0"
            if values:
                command = ac.commands["settings"]
                for name, value in values.items():
                    if name in command.parameters:
                        command.parameters[name].value = value
                if not await command.send():
                    raise RuntimeError("command_rejected")
        # O hOn demora um pouco a refletir a mudança; a app já mostrou o valor novo.
        await asyncio.sleep(1.5)
        await ac.update(force=True)
        return self.describe(ac)

    async def handle(self, request: Dict[str, Any]) -> Any:
        cmd = request.get("cmd")
        if cmd == "login":
            return await self.login(request["email"], request["password"])
        if cmd == "state":
            return await self.state(request["ac"])
        if cmd == "set":
            return await self.set(request["ac"], request.get("patch", {}))
        if cmd == "ping":
            return "pong"
        raise RuntimeError(f"unknown_command:{cmd}")


async def main() -> None:
    # A app passa o caminho do ficheiro de log. O logger de autenticação do pyhOn
    # fica desligado porque, quando o login falha, escreve páginas com tokens.
    if log_path := os.environ.get("HON_BRIDGE_LOG"):
        logging.basicConfig(
            filename=log_path, level=logging.INFO, encoding="utf-8",
            format="%(asctime)s %(levelname)s %(name)s: %(message)s",
        )
        logging.getLogger("pyhon.connection.auth").disabled = True
        logging.getLogger("pyhon.connection.handler.auth").disabled = True
    else:
        logging.disable(logging.CRITICAL)
    bridge = Bridge()
    loop = asyncio.get_running_loop()
    while True:
        line = await loop.run_in_executor(None, sys.stdin.readline)
        if not line:
            break
        if not line.strip():
            continue
        request: Dict[str, Any] = {}
        try:
            request = json.loads(line)
            result = await bridge.handle(request)
            response = {"id": request.get("id"), "ok": True, "result": result}
        except HonAuthenticationError:
            response = {"id": request.get("id"), "ok": False, "error": "auth_failed"}
        except Exception as error:  # pylint: disable=broad-except
            _LOGGER.exception("pedido %s falhou", request.get("cmd"))
            response = {"id": request.get("id"), "ok": False, "error": str(error) or type(error).__name__}
        sys.stdout.write(json.dumps(response) + "\n")
        sys.stdout.flush()
    await bridge.close()


if __name__ == "__main__":
    asyncio.run(main())
