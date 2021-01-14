console.log("CB begin");
let regexp = /^(\d+)*$/i;

let str = "012345678901234567890123456789z";

// will take a very long time (careful!)
console.log( regexp.test(str) );

console.log("CB end");
