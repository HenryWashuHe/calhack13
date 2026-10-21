FROM apify/actor-node:24-slim AS build
COPY --chown=myuser:myuser package.json package-lock.json ./
RUN npm ci --include=dev --ignore-scripts --no-audit --no-fund
COPY --chown=myuser:myuser tsconfig.json ./
COPY --chown=myuser:myuser src ./src
RUN npm run build && npm prune --omit=dev --ignore-scripts --no-audit --no-fund

FROM apify/actor-node:24-slim
COPY --from=build --chown=myuser:myuser /usr/src/app/node_modules ./node_modules
COPY --from=build --chown=myuser:myuser /usr/src/app/dist ./dist
COPY --chown=myuser:myuser package.json allowed-sources.json ./
CMD ["node", "dist/main.js"]
