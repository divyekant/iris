import { load } from "/home/user/iris-client/client/test/mount.mjs";
const fx = await load("/dev/fixtures.ts");
console.log(fx.items.map(i => `${i.id} ${i.form} ${i.status}`).join("\n"));
