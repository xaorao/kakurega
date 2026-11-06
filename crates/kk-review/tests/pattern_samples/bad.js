const cp = require("child_process");
const crypto = require("crypto");
function f(user, el) {
  eval(user);
  cp.exec(user);
  el.innerHTML = user;
  crypto.createHash("md5");
}
