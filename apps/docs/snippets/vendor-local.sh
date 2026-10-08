npm install @wordink/local@__LOCAL_VERSION__ esbuild
npx esbuild node_modules/@wordink/local/dist/index.js node_modules/@wordink/local/dist/worker.js --bundle --format=esm --outdir=wordink-local
