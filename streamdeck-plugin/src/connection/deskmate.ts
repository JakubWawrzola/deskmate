import type { ChannelClient, ChannelStatus } from "../channel/client.ts";
import {
  type DeskmateAction,
  type EntityState,
  type JsonRecord,
  ProtocolError,
  readActions,
  readEntities,
} from "../channel/protocol.ts";
import {
  ConnectionError,
  type DeskmateLink,
  type EntityInfo,
  type ServiceCall,
} from "./transport.ts";

/**
 * Home Assistant through the Deskmate app: every operation is one request on
 * the app's local channel, and the app does the talking to Home Assistant
 * with the address and token it already has.
 */
export class DeskmateTransport implements DeskmateLink {
  private readonly client: ChannelClient;

  constructor(client: ChannelClient) {
    this.client = client;
  }

  get status(): ChannelStatus {
    return this.client.status;
  }

  onStatus(listener: (status: ChannelStatus) => void): () => void {
    return this.client.onStatus(listener);
  }

  onState(listener: (state: EntityState) => void): () => void {
    return this.client.onState(listener);
  }

  setWanted(wanted: boolean): void {
    this.client.setWanted(wanted);
  }

  ensureConnected(): Promise<void> {
    return this.client.ensureConnected();
  }

  watch(entityIds: readonly string[]): void {
    this.client.setWatch(entityIds);
  }

  async toggle(entityId: string): Promise<void> {
    await this.client.request("toggle", { entity_id: entityId });
  }

  async callService(call: ServiceCall): Promise<void> {
    const params: JsonRecord = { service: call.service };
    if (call.entityId !== undefined) {
      params.entity_id = call.entityId;
    }
    if (call.data !== undefined) {
      params.data = call.data;
    }
    await this.client.request("call", params);
  }

  async listEntities(domains: readonly string[] | undefined): Promise<EntityInfo[]> {
    const body = await this.client.request("entities", domains ? { domains: [...domains] } : {});
    return this.read(body, readEntities);
  }

  async listActions(): Promise<DeskmateAction[]> {
    return this.read(await this.client.request("actions"), readActions);
  }

  async runAction(actionId: string): Promise<void> {
    await this.client.request("run", { action: actionId });
  }

  private read<T>(body: JsonRecord, reader: (body: JsonRecord) => T): T {
    try {
      return reader(body);
    } catch (error) {
      if (error instanceof ProtocolError) {
        throw new ConnectionError(
          "protocol",
          "The Deskmate app sent an answer the plugin cannot read",
        );
      }
      throw error;
    }
  }
}
