// Kill stars, client side: status templates read `../rules`' object
// variables. Levels 1..9 light one of nine pips per level (`Segments`),
// level 10 shows its number in gold,
// and every unit with a kill shows its kill count. The original rank
// insignia is hidden (`HideNative=Marks`), which a mod's module may do.
// `pips.shp` stands in for a star: ship your own SHP and name its file.
import { Client, Status, StatusTemplates, onRender } from "@vera20k/sdk/client";

let registered = false;

onRender((dt: f32, alpha: f32): void => {
  if (registered) return;
  registered = true;
  const templates: string[] = [
    "Name=Stars\nCategory=Vehicle,Infantry\nHideNative=Marks\n" +
      "When1=AtLeast,Var:Level,1\nWhen2=AtMost,Var:Level,9\n" +
      "Element1=Segments,-16,-36,4,4,4,0,9,Var:Level/9,pips.shp",
    "Name=Elite\nCategory=Vehicle,Infantry\nWhen1=AtLeast,Var:Level,10\n" +
      "Element1=Number,-16,-40,Var:Level,#FFD020",
    "Name=Kills\nCategory=Vehicle,Infantry\nWhen1=AtLeast,Var:Kills,1\n" +
      "Element1=Number,18,-44,Var:Kills,#FFFFFF",
  ];
  for (let i = 0; i < templates.length; i++) {
    if (StatusTemplates.set(templates[i]) != Status.Ok) {
      Client.log("kill-stars: template " + i.toString() + " was refused");
    }
  }
});
