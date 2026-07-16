import { Children, isValidElement, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

type MessageContentProps = {
  content: string;
};

type CodeElementProps = {
  className?: string;
  children?: ReactNode;
};

const components: Components = {
  p: ({ node: _node, ...props }) => (
    <p className="message-paragraph" {...props} />
  ),
  h1: ({ node: _node, ...props }) => (
    <h1 className="message-heading message-heading-1" {...props} />
  ),
  h2: ({ node: _node, ...props }) => (
    <h2 className="message-heading message-heading-2" {...props} />
  ),
  h3: ({ node: _node, ...props }) => (
    <h3 className="message-heading message-heading-3" {...props} />
  ),
  h4: ({ node: _node, ...props }) => (
    <h4 className="message-heading message-heading-4" {...props} />
  ),
  h5: ({ node: _node, ...props }) => (
    <h5 className="message-heading message-heading-5" {...props} />
  ),
  h6: ({ node: _node, ...props }) => (
    <h6 className="message-heading message-heading-6" {...props} />
  ),
  ul: ({ node: _node, ...props }) => (
    <ul className="message-list message-list-unordered" {...props} />
  ),
  ol: ({ node: _node, ...props }) => (
    <ol className="message-list message-list-ordered" {...props} />
  ),
  li: ({ node: _node, ...props }) => (
    <li className="message-list-item" {...props} />
  ),
  blockquote: ({ node: _node, ...props }) => (
    <blockquote className="message-blockquote" {...props} />
  ),
  table: ({ node: _node, ...props }) => (
    <div
      className="message-table-scroll"
      role="region"
      aria-label="Scrollable table"
      tabIndex={0}
    >
      <table className="message-table" {...props} />
    </div>
  ),
  th: ({ node: _node, ...props }) => (
    <th className="message-table-heading" {...props} />
  ),
  td: ({ node: _node, ...props }) => (
    <td className="message-table-cell" {...props} />
  ),
  pre: ({ node: _node, children, ...props }) => {
    const child = Children.toArray(children)[0];
    if (isValidElement<CodeElementProps>(child)) {
      const language = /language-([\w-]+)/.exec(
        child.props.className ?? "",
      )?.[1];
      return (
        <div className="message-code-block">
          {language && (
            <span className="message-code-language">{language}</span>
          )}
          <pre
            className="message-code-pre"
            aria-label={language ? `${language} code` : "Code"}
            {...props}
          >
            <code className={child.props.className}>
              {child.props.children}
            </code>
          </pre>
        </div>
      );
    }
    return (
      <pre className="message-code-pre" {...props}>
        {children}
      </pre>
    );
  },
  code: ({ node: _node, ...props }) => (
    <code className="message-inline-code" {...props} />
  ),
  a: ({ node: _node, ...props }) => (
    <a
      className="message-link"
      {...props}
      target="_blank"
      rel="noreferrer noopener"
    />
  ),
  hr: ({ node: _node, ...props }) => (
    <hr className="message-divider" {...props} />
  ),
};

export function MessageContent({ content }: MessageContentProps) {
  return (
    <div className="message-content">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {content}
      </ReactMarkdown>
    </div>
  );
}
