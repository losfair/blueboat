// Test image from https://github.com/Brooooooklyn/canvas#usage
function generateTestImage(): Uint8Array {
  const canvas = new Graphics.Canvas(300, 320)
  const ctx = canvas.getContext('2d')

  ctx.lineWidth = 10
  ctx.strokeStyle = '#03a9f4'
  ctx.fillStyle = '#03a9f4'

  ctx.font = "15px";
  ctx.fillText("Hello, world!", 10, 10);

  ctx.font = "30px roboto, sans-serif";
  ctx.fillStyle = '#837022'
  ctx.fillText("Hello 测试", 10, 30);

  ctx.fillStyle = '#03a9f4'

  // Wall
  ctx.strokeRect(75, 140, 150, 110)

  // Door
  ctx.fillRect(130, 190, 40, 60)

  // Roof
  ctx.beginPath()
  ctx.moveTo(50, 140)
  ctx.lineTo(150, 60)
  ctx.lineTo(250, 140)
  ctx.closePath()
  ctx.stroke();
  const buffer = canvas.encode({
    type: "png",
    quality: 90,
  });
  return buffer;
}

const constImage = generateTestImage();

Router.get("/image", req => new Response(constImage, {
  headers: {
    "Content-Type": "image/png",
  },
}));

Router.get("/rt_image", req => new Response(generateTestImage(), {
  headers: {
    "Content-Type": "image/png",
  },
}));

const tigerSvg = new TextDecoder().decode(Package["tiger.svg"]);
Router.get("/tiger.svg", req => {
  return new Response(tigerSvg, {
    headers: {
      "Content-Type": "image/svg+xml",
    },
  })
});

Router.get("/tiger", req => {
  const smallCvs = new Graphics.Canvas(200, 100);
  const svgCtx = smallCvs.getContext('2d');
  svgCtx.fillStyle = "#ffffff";
  svgCtx.fillRect(0, 0, 200, 100);
  svgCtx.renderSvg(tigerSvg, {
    type: "Size",
    width: 200,
    height: 100
  });
  svgCtx.fillStyle = "#000000";
  svgCtx.fillText("Tiger", 150, 80);
  const cvs = new Graphics.Canvas(450, 300);
  const ctx = cvs.getContext("2d");
  ctx.drawImage(smallCvs, 10, 10);
  ctx.drawImage(smallCvs, 220, 10);
  ctx.drawImage(smallCvs, 10, 150);
  ctx.drawImage(smallCvs, 220, 150);
  return new Response(cvs.encode({
    type: "png",
  }), {
    headers: {
      "Content-Type": "image/png",
    },
  });
})