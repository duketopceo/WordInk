// Loaded by every docs page. Only the home page has the live demo, so only it pulls in the SDK and
// the local engine.
if (document.getElementById("demo")) {
  void import("./demo.js");
}
