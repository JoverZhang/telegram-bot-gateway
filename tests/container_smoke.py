#!/usr/bin/env python3
"""Check the production image without network access or real Telegram credentials."""

import argparse
import json
import pathlib
import subprocess
import tempfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image", required=True)
    args = parser.parse_args()
    name = "tbg-smoke-" + uuid.uuid4().hex[:12]
    report = ROOT / "target/e2e"
    report.mkdir(parents=True, exist_ok=True)

    def docker(*args, check=True):
        return subprocess.run(
            ["docker", *args], capture_output=True, text=True, check=check, timeout=30
        )

    with tempfile.TemporaryDirectory(prefix="tbg-container-") as directory:
        root = pathlib.Path(directory)
        config, data = root / "config", root / "data"
        config.mkdir()
        data.mkdir()
        (config / "server.yaml").write_text(
            'listen: "127.0.0.1:18473"\ndata_dir: "/var/lib/tbg"\n'
            'telegram:\n  bot_token: "UNUSABLE_TEST_TOKEN"\nadmins: []\n'
        )
        (config / "client.yaml").write_text('host: "127.0.0.1"\nport: 18473\n')
        try:
            docker(
                "run",
                "-d",
                "--name",
                name,
                "--network",
                "none",
                "-v",
                f"{config}:/root/.config/tbg:ro",
                "-v",
                f"{data}:/var/lib/tbg",
                args.image,
            )
            for _ in range(50):
                result = docker(
                    "exec",
                    name,
                    "tbg",
                    "agent",
                    "register",
                    "--name",
                    "smoke",
                    check=False,
                )
                if result.returncode == 0:
                    break
                time.sleep(0.1)
            assert result.returncode == 0, result.stderr
            assert json.loads(result.stdout) == {"name": "smoke"}
            docker("restart", "--time", "10", name)
            result = docker("exec", name, "tbg", "--agent", "smoke", "whoami")
            assert json.loads(result.stdout) == {"name": "smoke"}
            assert (data / "gateway.sqlite").exists()
            (report / "container.json").write_text(
                json.dumps(
                    {
                        "image": args.image,
                        "passed": [
                            "production startup",
                            "CLI over HTTP",
                            "restart persistence",
                        ],
                        "network": "none",
                    },
                    indent=2,
                )
                + "\n"
            )
            print(
                "Container startup, CLI and restart persistence passed (network disabled)."
            )
        finally:
            logs = docker("logs", name, check=False)
            (report / "container.log").write_text(logs.stdout + logs.stderr)
            docker("rm", "-f", name, check=False)


if __name__ == "__main__":
    main()
