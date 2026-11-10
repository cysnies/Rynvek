// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception

import { describe, expect, it } from "vitest";
import {
  AGENT_TOOL_NAMES,
  buildConfigureSignalBeacon,
  buildIssueStrategicIntent,
  buildPlaceBuilding,
  buildQueryWorldObservation,
  validateToolBatch,
} from "../src/index.js";

describe("@rynvek/agent-sdk", () => {
  it("exports all 9 frozen ToolBroker tool names and validates batches", () => {
    expect(AGENT_TOOL_NAMES.length).toBe(9);
    const batch = [
      buildQueryWorldObservation(true),
      buildPlaceBuilding({ buildingKind: "bld.spawning_vat", tileX: 36, tileY: 32 }),
      buildIssueStrategicIntent({
        archetype: "unit.striker",
        orderKind: "AttackMove",
        targetX: 128,
        targetY: 128,
      }),
      buildConfigureSignalBeacon({
        beaconId: "5",
        channel: "field.signal_b",
        strength: 35,
        modulators: { da: 0.5, serotonin: -0.2, ne: 0.8, ach: 0.4 },
      }),
    ];
    expect(() => validateToolBatch(batch)).not.toThrow();
  });
});
