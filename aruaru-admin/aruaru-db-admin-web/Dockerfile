# Web 版ビルド & 配信
FROM node:20-alpine AS build
WORKDIR /app
COPY . .
RUN corepack enable && pnpm install --frozen-lockfile=false && pnpm --filter aruaru-db-admin-web build
FROM nginx:alpine
COPY --from=build /app/aruaru-admin/aruaru-db-admin-web/dist /usr/share/nginx/html
COPY aruaru-admin/aruaru-db-admin-web/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
