# Built by the eval workflow from evals/image/Dockerfile.
FROM craft-eval:local
COPY --chown=node:node repo/ /app/
USER node
RUN git -C /app init -q && git -C /app add -A && git -C /app commit -q -m "Initial checkout service"
USER root
WORKDIR /app
