// Enumerate normal app windows across every output; no desktop pointer grab.
var windows = workspace.windowList().filter(function(w) {
    return w.normalWindow && !w.deleted && !w.skipSwitcher;
}).map(function(w) {
    return {uuid:String(w.internalId), title:String(w.caption), output:w.output ? String(w.output.name) : "Unknown display"};
});
callDBus(__BUS__, '/org/wayvr/XS', 'org.wayvr.XS.WindowInput', 'Choices', JSON.stringify(windows));
