// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: CC0-1.0
import { Kernel } from "@rynvek/rules";

// 真实AST编译输入，不是作者自报的已编译IR附件。
export default Kernel.define(
  "有来源AST转向",
  { reads: { "param.thrust": "Q8_8" }, writes: { "state.accel_x": "Q8_8" } },
  (k) => {
    const thrust = k.load("param.thrust");
    k.write("state.accel_x", thrust.mul(k.q8(0.5)));
  },
);
