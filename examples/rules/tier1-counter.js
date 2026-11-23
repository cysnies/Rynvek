"use strict";
function tick(input) {
    return { writes: input.entities.map(e => ({ id: e.id, values: {
                "state.total": e.values["state.total"] + e.values["param.rate"]
            } })) };
}
