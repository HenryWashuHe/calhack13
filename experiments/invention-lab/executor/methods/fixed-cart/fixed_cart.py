"""Baseline control: submits one fixed two-wheel cart without experimenting.

Hand-written by Henry's Claude as the acceptance control slot. CONTROL ONLY.
"""
import json
import sys

DESIGN = {
    "language": 1,
    "payload_on": "deck",
    "bodies": [
        {"shape": "rect", "id": "deck", "x": -2.0, "y": 0.6, "w": 3.6, "h": 0.3, "material": "wood"},
        {"shape": "circle", "id": "w1", "x": -3.5, "y": 0.45, "r": 0.3, "material": "rubber"},
        {"shape": "circle", "id": "w2", "x": -0.5, "y": 0.45, "r": 0.3, "material": "rubber"},
    ],
    "joints": [
        {"a": "deck", "b": "w1", "kind": "hinge", "x": -3.5, "y": 0.45, "motor_speed": -5.0},
        {"a": "deck", "b": "w2", "kind": "hinge", "x": -0.5, "y": 0.45, "motor_speed": -5.0},
    ],
}

sys.stdout.write(json.dumps({"op": "submit", "design": DESIGN}) + "\n")
sys.stdout.flush()
reply = json.loads(sys.stdin.readline() or "{}")
sys.exit(0 if reply.get("ok") else 1)
