three.js 0.160.0 (MIT, see LICENSE), copied unchanged from the `three@0.160.0` npm package so the try-it page works with no internet:
`build/three.module.min.js` as `three.module.min.js`, and `examples/jsm/{loaders/GLTFLoader.js,controls/OrbitControls.js,utils/BufferGeometryUtils.js}` as `jsm/...`.
The page's import map (index.html) points `three` and `three/addons/` here; server.js serves exactly these files.
